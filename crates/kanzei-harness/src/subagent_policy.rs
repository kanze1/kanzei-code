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

/// 只描述委派决策与结果责任；宿主专属协议由实际 task 工具提供。
pub const DELEGATION_GUIDANCE: &str = "根据任务的独立性、上下文需求和可用能力决定是否委派。说明子任务的目标、范围、必要上下文和预期结果，避免重复或相互冲突的分工。子任务运行期间推进独立工作；根据实际返回的结果判断完成情况，并在整合前检查相关改动与证据。";

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
