//! Conversation-level delegation preference. Scheduling and isolation remain runtime concerns.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentMode {
    Off,
    #[default]
    Auto,
    Ultra,
}

impl SubagentMode {
    pub fn enabled(self) -> bool {
        self != Self::Off
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Auto => "auto",
            Self::Ultra => "ultra",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "off" => Some(Self::Off),
            "auto" => Some(Self::Auto),
            "ultra" => Some(Self::Ultra),
            _ => None,
        }
    }
    pub fn concurrency(self, limits: &crate::config::Limits) -> usize {
        match self {
            Self::Off => 0,
            Self::Auto => limits.subagent_auto_concurrency(),
            Self::Ultra => limits.subagent_ultra_concurrency(),
        }
    }
    pub fn guidance(self) -> &'static str {
        match self {
            Self::Off => "Subagents are disabled for this conversation. Complete the work yourself using the available tools.",
            Self::Auto => "Subagent preference: Auto. Delegate when an independent task meaningfully reduces latency or context load; doing the work yourself is equally valid. Keep concurrency modest and avoid duplicate assignments.",
            Self::Ultra => "Subagent preference: Ultra. Actively consider delegation for independent research, implementation, tests, or review. Choose the number of workers from the actual independent work and available capacity. Simple or tightly coupled work can stay with you. No minimum worker count or mandatory scout/review stages.",
        }
    }
}

pub const DELEGATION_GUIDANCE: &str = "You own decomposition, dependencies, and the decision to review. Give each task a clear goal, scope, expected evidence, and relevant context. Continue useful independent work while children run; wait when their result is actually needed. Use task dependencies for candidate-based verification, and inspect results before integrating them. Delegation is not completion. Use fresh context by default; fork only when needed. A child works on its assigned task and cannot delegate recursively. Independent read-only tasks share code; tasks with dependencies inspect an isolated candidate checkout. Writable desktop tasks use isolated Git worktrees. Avoid overlapping edits and review the actual candidate version. Do not prescribe a fixed workflow merely because subagents are available.";

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modes_roundtrip_and_have_independent_limits() {
        let limits = crate::config::Limits::default();
        for mode in [SubagentMode::Off, SubagentMode::Auto, SubagentMode::Ultra] {
            assert_eq!(SubagentMode::parse(mode.as_str()), Some(mode));
            assert_eq!(
                serde_json::from_str::<SubagentMode>(&serde_json::to_string(&mode).unwrap())
                    .unwrap(),
                mode
            );
        }
        assert_eq!(SubagentMode::Auto.concurrency(&limits), 2);
        assert_eq!(SubagentMode::Ultra.concurrency(&limits), 8);
        assert_eq!(SubagentMode::Off.concurrency(&limits), 0);
        assert_eq!(SubagentMode::default(), SubagentMode::Auto);
    }
}
