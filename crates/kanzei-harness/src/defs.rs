//! Agent / Command / Skill 定义与 Profile 标识。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProfileKind {
    Dev,
    Research,
    /// 只读分析档位(R-102):read/glob/grep/task 放行,写与命令硬拒绝。
    Readonly,
}

impl std::str::FromStr for ProfileKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "dev" => Ok(ProfileKind::Dev),
            "research" => Ok(ProfileKind::Research),
            "readonly" => Ok(ProfileKind::Readonly),
            other => Err(format!("unknown profile `{other}` (dev|research|readonly)")),
        }
    }
}

/// agent 属于哪个 profile;All 表示两个模式都可用。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProfileScope {
    Dev,
    Research,
    #[default]
    All,
}

impl ProfileScope {
    pub fn includes(&self, profile: ProfileKind) -> bool {
        match self {
            ProfileScope::All => true,
            ProfileScope::Dev => profile == ProfileKind::Dev,
            ProfileScope::Research => profile == ProfileKind::Research,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentMode {
    #[default]
    Primary,
    Subagent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDef {
    pub name: String,
    #[serde(default)]
    pub profile: ProfileScope,
    /// 模型引用:"primary" | "fast"(角色)或 "provider:model"(直指)。
    #[serde(default = "default_model_ref")]
    pub model: String,
    #[serde(default)]
    pub mode: AgentMode,
    #[serde(default = "default_steps")]
    pub steps: u32,
    /// 系统提示词正文(markdown body)。
    #[serde(default)]
    pub system: String,
}

/// task 子代理未指定预算时的默认轮数。主代理没有隐式的 32 步截断。
pub const DEFAULT_AGENT_STEPS: u32 = 32;

fn default_model_ref() -> String {
    "primary".into()
}

fn default_steps() -> u32 {
    0
}

/// 将 agent 定义中的轮数转换为运行器实际使用的步数预算。
///
/// 0 表示跟随运行角色：主代理不设步数上限，task 子代理使用 32 步预算。
/// 非零值是用户显式配置，两种角色均保留。
pub fn effective_agent_steps(steps: u32, mode: AgentMode) -> u32 {
    if steps == 0 && mode == AgentMode::Subagent {
        DEFAULT_AGENT_STEPS
    } else {
        steps
    }
}

#[cfg(test)]
mod tests {
    use super::{effective_agent_steps, AgentDef, AgentMode, DEFAULT_AGENT_STEPS};

    #[test]
    fn 仅子代理的零轮数使用有限默认上限() {
        assert_eq!(effective_agent_steps(0, AgentMode::Primary), 0);
        assert_eq!(
            effective_agent_steps(0, AgentMode::Subagent),
            DEFAULT_AGENT_STEPS
        );
        const { assert!(DEFAULT_AGENT_STEPS > 0) };
    }

    #[test]
    fn 显式轮数保持原值() {
        for mode in [AgentMode::Primary, AgentMode::Subagent] {
            assert_eq!(effective_agent_steps(7, mode), 7);
        }
    }

    #[test]
    fn 未声明轮数由角色决定预算() {
        for (mode, expected) in [("primary", 0), ("subagent", DEFAULT_AGENT_STEPS)] {
            let agent: AgentDef = serde_json::from_value(serde_json::json!({
                "name": "custom", "mode": mode
            }))
            .unwrap();
            assert_eq!(effective_agent_steps(agent.steps, agent.mode), expected);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDef {
    pub name: String,
    pub description: String,
    /// SKILL.md 路径;名称/描述/路径索引进提示词,正文由模型按此路径用 read 读取
    /// (没有 skill 工具)。
    pub path: std::path::PathBuf,
}
