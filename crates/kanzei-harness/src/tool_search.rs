//! Deferred discovery: a bounded directory, validated paging and schema-loading receipts.
use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{Tool, ToolConcurrency, ToolCtx, ToolOutput};

pub const TOOL_SEARCH: &str = "tool_search";
const MAX_RESULTS: usize = 10;
const DEFAULT_LIMIT: usize = 5;
const CATALOG_CHAR_BUDGET: usize = 4000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchInput {
    query: String,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    offset: usize,
}

fn default_limit() -> usize {
    DEFAULT_LIMIT
}

impl SearchInput {
    pub fn parse(input: &Value) -> Result<Self, Box<ToolOutput>> {
        let mut input: Self = serde_json::from_value(input.clone()).map_err(|error| {
            Box::new(ToolOutput::needs_correction(
                "TOOL_SEARCH_INPUT",
                error.to_string(),
            ))
        })?;
        input.query = input.query.trim().to_owned();
        if !(1..=MAX_RESULTS).contains(&input.limit) {
            return Err(Box::new(ToolOutput::needs_correction(
                "TOOL_SEARCH_LIMIT",
                "limit must be an integer from 1 to 10.",
            )));
        }
        if let Some(selection) = input.query.strip_prefix("select:") {
            let names: HashSet<_> = selection
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .collect();
            if names.is_empty() || names.len() > MAX_RESULTS || input.offset != 0 {
                return Err(Box::new(ToolOutput::needs_correction(
                    "TOOL_SEARCH_SELECTION",
                    "select: requires 1 to 10 distinct names and offset=0.",
                )));
            }
        }
        Ok(input)
    }
}

/// Only names and page metadata are returned to history; specs belong to the next request.
pub struct SearchResult {
    pub selected: Vec<Arc<dyn Tool>>,
    pub already_available: Vec<String>,
    pub unknown: Vec<String>,
    pub matched: usize,
    pub next_offset: Option<usize>,
}

pub struct ToolSearchTool;

#[async_trait]
impl Tool for ToolSearchTool {
    fn read_only_support(&self) -> crate::tool::ReadOnlySupport {
        crate::tool::ReadOnlySupport::ReadOnly
    }
    fn name(&self) -> &'static str {
        TOOL_SEARCH
    }
    fn description(&self) -> String {
        "Load deferred tool schemas by keyword or exact select:name1,name2; an empty query lists tools. Returns a loading receipt and next_offset.".into()
    }
    fn input_schema(&self) -> Value {
        json!({
            "type":"object", "required":["query"],
            "properties":{
                "query":{"type":"string","description":"Keywords, empty string for a directory page, or select:name1,name2 (up to 10 distinct names)."},
                "limit":{"type":"integer","minimum":1,"maximum":MAX_RESULTS,"default":DEFAULT_LIMIT,"description":"Page size for keywords or an empty query."},
                "offset":{"type":"integer","minimum":0,"default":0,"description":"Use next_offset for the same query; exact selection requires 0."}
            },
            "additionalProperties":false
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

/// A directory summary is navigation, not a second full tool description.
pub fn one_line(description: &str) -> String {
    let description = description.trim();
    let end = [". ", "。", "\n"]
        .iter()
        .filter_map(|separator| description.find(separator))
        .min()
        .unwrap_or(description.len());
    let sentence = description[..end].trim();
    let mut chars = sentence.chars();
    let mut text: String = chars.by_ref().take(120).collect();
    if chars.next().is_some() {
        text.pop();
        text.push('…');
    }
    text
}

/// Page over the full ranked match set before classifying loaded tools, so offsets remain stable.
pub fn search(
    input: &SearchInput,
    deferred: &[Arc<dyn Tool>],
    available_names: &HashSet<String>,
) -> SearchResult {
    let mut result = SearchResult {
        selected: Vec::new(),
        already_available: Vec::new(),
        unknown: Vec::new(),
        matched: 0,
        next_offset: None,
    };
    if let Some(selection) = input.query.strip_prefix("select:") {
        let mut seen = HashSet::new();
        for name in selection
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            if !seen.insert(name) {
                continue;
            }
            if available_names.contains(name) {
                result.already_available.push(name.to_owned());
            } else if let Some(tool) = deferred.iter().find(|tool| tool.name() == name) {
                result.selected.push(Arc::clone(tool));
            } else {
                result.unknown.push(name.to_owned());
            }
        }
        result.matched = result.selected.len() + result.already_available.len();
        return result;
    }
    let mut terms: Vec<String> = input
        .query
        .split_whitespace()
        .map(str::to_lowercase)
        .collect();
    terms.sort();
    terms.dedup();
    let mut ranked: Vec<_> = deferred
        .iter()
        .enumerate()
        .filter_map(|(index, tool)| {
            let name = tool.name().to_lowercase();
            let description = tool.description().to_lowercase();
            let score: usize = if terms.is_empty() {
                1
            } else {
                terms
                    .iter()
                    .map(|term| {
                        usize::from(name.contains(term)) * 3
                            + usize::from(description.contains(term))
                    })
                    .sum()
            };
            (score > 0).then_some((score, index, tool))
        })
        .collect();
    ranked.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    result.matched = ranked.len();
    let end = input.offset.saturating_add(input.limit).min(ranked.len());
    result.next_offset = (end < ranked.len()).then_some(end);
    for (_, _, tool) in ranked.into_iter().skip(input.offset).take(input.limit) {
        if available_names.contains(tool.name()) {
            result.already_available.push(tool.name().to_owned());
        } else {
            result.selected.push(Arc::clone(tool));
        }
    }
    result
}

/// Stable registration order; whole rows and the omission receipt share one character budget.
pub fn render_catalog(deferred: &[Arc<dyn Tool>]) -> Option<String> {
    if deferred.is_empty() {
        return None;
    }
    let total = deferred.len();
    let mut catalog=format!("<deferred-tools total=\"{total}\">\nLoad schemas with tool_search: keywords or query=\"select:name1,name2\". Empty query lists a page; offset continues it. Schemas join the next request and remain loaded for this run; direct calls also auto-load.\n");
    let footer = |listed| {
        format!("Listed {listed}/{total}. More: tool_search query=\"\" offset={listed}.\n</deferred-tools>")
    };
    let reserved = footer(total).chars().count();
    let mut used = catalog.chars().count();
    let mut listed = 0;
    for tool in deferred {
        let row = format!("- {} — {}\n", tool.name(), one_line(&tool.description()));
        let chars = row.chars().count();
        if used + chars + reserved > CATALOG_CHAR_BUDGET {
            break;
        }
        catalog.push_str(&row);
        used += chars;
        listed += 1;
    }
    if listed < total {
        catalog.push_str(&footer(listed));
    } else {
        catalog.push_str("</deferred-tools>");
    }
    Some(catalog)
}

pub fn render_result(result: &SearchResult) -> ToolOutput {
    let content = json!({
        "loaded":result.selected.iter().map(|tool|tool.name()).collect::<Vec<_>>(),
        "already_available":result.already_available,"unknown":result.unknown,
        "matched":result.matched,"next_offset":result.next_offset
    })
    .to_string();
    if result.unknown.is_empty() {
        ToolOutput::ok(content)
    } else {
        ToolOutput::needs_correction("TOOL_SEARCH_UNKNOWN", content)
    }
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
            json!({"type":"object","properties":{"SCHEMA_CANARY":{"type":"string"}}})
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
    fn request(query: &str, limit: usize, offset: usize) -> SearchInput {
        SearchInput::parse(&json!({"query":query,"limit":limit,"offset":offset})).unwrap()
    }
    #[test]
    fn selection_deduplicates_preserves_order_and_reports_partial_loading() {
        let result = search(
            &request("select:background,read,missing,background", 5, 0),
            &tools(),
            &HashSet::from(["read".to_owned()]),
        );
        assert_eq!(result.selected[0].name(), "background");
        assert_eq!(result.already_available, ["read"]);
        assert_eq!(result.unknown, ["missing"]);
        let output = render_result(&result);
        assert_eq!(output.code.as_deref(), Some("TOOL_SEARCH_UNKNOWN"));
        let receipt: Value = serde_json::from_str(&output.content).unwrap();
        assert_eq!(receipt["loaded"], json!(["background"]));
        assert!(!output.content.contains("SCHEMA_CANARY"));
    }
    #[test]
    fn pagination_keeps_its_order_when_previous_pages_are_loaded() {
        let tools = tools();
        let mut available = HashSet::new();
        let first = search(&request("process", 1, 0), &tools, &available);
        assert_eq!(first.selected[0].name(), "process");
        assert_eq!(first.next_offset, Some(1));
        available.insert("process".to_owned());
        let second = search(
            &request("process", 1, first.next_offset.unwrap()),
            &tools,
            &available,
        );
        assert_eq!(second.selected[0].name(), "read_output");
        assert_eq!(second.matched, 3);
        let repeat = search(&request("process", 1, 0), &tools, &available);
        assert!(repeat.selected.is_empty());
        assert_eq!(repeat.already_available, ["process"]);
        assert_eq!(repeat.next_offset, Some(1));
        let last = search(&request("", 2, 2), &tools, &available);
        assert_eq!(last.selected[0].name(), "background");
        assert_eq!(last.next_offset, None);
        let beyond = search(&request("", 2, usize::MAX), &tools, &available);
        assert!(beyond.selected.is_empty());
        assert_eq!(beyond.next_offset, None);
    }
    #[test]
    fn invalid_inputs_are_rejected_instead_of_clamped_or_partially_loaded() {
        for input in [
            json!({}),
            json!({"query":"x","limit":0}),
            json!({"query":"x","limit":11}),
            json!({"query":"x","offset":-1}),
            json!({"query":"x","offset":1.5}),
            json!({"query":"x","unknown":true}),
            json!({"query":"select:"}),
            json!({"query":"select:process","offset":1}),
            json!({"query":"select:a,b,c,d,e,f,g,h,i,j,k"}),
        ] {
            assert!(SearchInput::parse(&input).is_err(), "{input}");
        }
    }
    #[test]
    fn catalog_budget_includes_unicode_rows_and_an_exact_continuation() {
        let tools: Vec<Arc<dyn Tool>> = (0..80)
            .map(|index| {
                Arc::new(DummyTool {
                    name: Box::leak(format!("tool_{index}").into_boxed_str()),
                    description: Box::leak("资料".repeat(100).into_boxed_str()),
                }) as Arc<dyn Tool>
            })
            .collect();
        let catalog = render_catalog(&tools).unwrap();
        assert!(catalog.chars().count() <= CATALOG_CHAR_BUDGET);
        assert!(catalog.ends_with("</deferred-tools>"));
        let listed = catalog
            .lines()
            .filter(|line| line.starts_with("- "))
            .count();
        assert!(listed < tools.len());
        assert!(catalog.contains(&format!("offset={listed}")));
        let page = search(&request("", 1, listed), &tools, &HashSet::new());
        assert_eq!(page.selected[0].name(), tools[listed].name());
        assert_eq!(one_line("  first sentence. other"), "first sentence");
        assert!(render_catalog(&[]).is_none());
    }
}
