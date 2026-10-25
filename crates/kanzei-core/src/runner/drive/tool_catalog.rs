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
    let request = match kanzei_harness::tool_search::SearchInput::parse(input) {
        Ok(request) => request,
        Err(output) => return *output,
    };
    let deferred = snapshot.deferred_tools();
    let available_names: std::collections::HashSet<String> =
        specs.iter().map(|spec| spec.name.clone()).collect();
    let mut result = kanzei_harness::tool_search::search(&request, &deferred, &available_names);
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

#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_harness::{
        rule, Component, Effect, Harness, HarnessDraft, KanzeiConfig, ProfileKind, ResolveCtx,
        ToolConcurrency, ToolCtx, ToolOutput,
    };
    use serde_json::{json, Value};
    use std::{path::PathBuf, sync::Arc};

    struct DeferredFixture;

    struct FixtureTool(&'static str);

    #[async_trait::async_trait]
    impl Tool for FixtureTool {
        fn name(&self) -> &'static str {
            self.0
        }
        fn description(&self) -> String {
            format!("{} test tool.", self.0)
        }
        fn input_schema(&self) -> Value {
            json!({"type":"object", "properties":{}})
        }
        fn concurrency(&self, _input: &Value, ctx: &ToolCtx) -> ToolConcurrency {
            ToolConcurrency::shared_worktree(ctx)
        }
        async fn execute(&self, _input: Value, _ctx: &ToolCtx) -> ToolOutput {
            ToolOutput::ok("unused")
        }
    }

    impl Component for DeferredFixture {
        fn contribute(&self, draft: &mut HarnessDraft, _ctx: &ResolveCtx) -> anyhow::Result<()> {
            draft.tools.insert(
                kanzei_harness::TOOL_SEARCH,
                Arc::new(FixtureTool(kanzei_harness::TOOL_SEARCH)),
            );
            draft
                .tools
                .insert("process", Arc::new(FixtureTool("process")));
            draft.deferred_tools.insert("process".into());
            draft
                .permissions
                .push(rule(kanzei_harness::TOOL_SEARCH, "*", Effect::Allow));
            Ok(())
        }
    }

    fn deferred_snapshot() -> Arc<HarnessSnapshot> {
        let root = PathBuf::from("C:/r364-deferred-process-test");
        let ctx = ResolveCtx {
            profile: ProfileKind::DevAuto,
            cwd: root.clone(),
            project_root: root,
            config: Arc::new(KanzeiConfig::default()),
        };
        let mut harness = Harness::default();
        harness.add(DeferredFixture);
        harness.resolve(&ctx).unwrap()
    }

    #[test]
    fn search_loads_one_spec_and_returns_only_a_receipt() {
        let snapshot = deferred_snapshot();
        let mut specs = Vec::new();
        let mut report = Vec::new();
        let output = run_tool_search(
            &snapshot,
            &json!({"query":"select:process"}),
            &mut specs,
            &mut report,
        );
        assert!(!output.is_error, "{}", output.content);
        let receipt: Value = serde_json::from_str(&output.content).unwrap();
        assert_eq!(receipt["loaded"], json!(["process"]));
        assert!(!output.content.contains("input_schema"));
        assert!(!output.content.contains("description"));
        assert_eq!(specs.len(), 1);
        assert_eq!(
            report,
            vec![("tools/loaded:process".into(), specs[0].char_len())]
        );
        let repeated = run_tool_search(
            &snapshot,
            &json!({"query":"select:process"}),
            &mut specs,
            &mut report,
        );
        let receipt: Value = serde_json::from_str(&repeated.content).unwrap();
        assert_eq!(receipt["already_available"], json!(["process"]));
        assert_eq!(specs.len(), 1);
        assert_eq!(report.len(), 1);
    }

    #[test]
    fn invalid_search_never_mutates_loaded_specs_or_billing() {
        let snapshot = deferred_snapshot();
        for input in [
            json!({"query":"select:process","limit":0}),
            json!({"query":"select:process","extra":true}),
            json!({"query":"select:process","offset":1}),
        ] {
            let mut specs = Vec::new();
            let mut report = Vec::new();
            assert!(run_tool_search(&snapshot, &input, &mut specs, &mut report).is_error);
            assert!(specs.is_empty());
            assert!(report.is_empty());
        }
    }

    #[test]
    fn direct_calls_still_load_deferred_specs_without_a_search_round() {
        let snapshot = deferred_snapshot();
        let mut specs = Vec::new();
        let mut report = Vec::new();
        auto_load_deferred_tool(&snapshot, "process", &mut specs, &mut report);
        auto_load_deferred_tool(&snapshot, "process", &mut specs, &mut report);
        assert_eq!(specs.len(), 1);
        assert_eq!(report.len(), 1);
    }
}
