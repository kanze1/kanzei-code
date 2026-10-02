//! Optional durable delegation host. The runner stays independent of Git/UI/storage.
use kanzei_harness::ToolOutput;
use kanzei_llm::{ReasoningEffort, ToolSpec};
use std::{future::Future, pin::Pin, sync::Arc};

pub type DelegationFuture = Pin<Box<dyn Future<Output = ToolOutput> + Send>>;

pub trait DelegationHost: Send + Sync {
    fn spec(&self) -> ToolSpec;
    fn execute(&self, call_id: String, input: serde_json::Value) -> DelegationFuture;
    fn set_parent_context(&self, _messages: &[kanzei_llm::Message]) {}
    fn has_updates(&self) -> bool {
        false
    }
    fn stop_all(&self) {}
}

#[derive(Clone, Default)]
pub struct SubagentOptions {
    pub reasoning: ReasoningEffort,
    pub fast_context_limit: Option<u64>,
    pub primary_context_limit: Option<u64>,
    pub ask_policy: Option<super::AskPolicy>,
    pub host: Option<Arc<dyn DelegationHost>>,
}

/// An explicit call override wins; otherwise honor the selected role.
pub fn subagent_model_tier<'a>(input: &'a serde_json::Value, role_model: &'a str) -> &'a str {
    input
        .get("model")
        .and_then(|value| value.as_str())
        .unwrap_or(role_model)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn planning_inherits_primary_unless_explicitly_overridden() {
        assert_eq!(
            subagent_model_tier(&serde_json::json!({"agent":"plan"}), "primary"),
            "primary"
        );
        assert_eq!(
            subagent_model_tier(&serde_json::json!({"model":"fast"}), "primary"),
            "fast"
        );
        assert_eq!(subagent_model_tier(&serde_json::json!({}), "fast"), "fast");
    }
}
