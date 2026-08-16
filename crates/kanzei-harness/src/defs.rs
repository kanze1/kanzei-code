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

/// 默认单次运行最多执行的模型/工具轮数。
///
/// 旧配置和旧 agent 定义里的 `steps = 0` 仍会被读取，但不再代表无限运行；
/// 统一通过 `effective_agent_steps` 转换成这个有限上限，避免一次误触发把
/// provider 请求和工具调用无限放大。
pub const DEFAULT_AGENT_STEPS: u32 = 32;

fn default_model_ref() -> String {
    "primary".into()
}

fn default_steps() -> u32 {
    DEFAULT_AGENT_STEPS
}

/// 将 agent 定义中的轮数转换为运行器实际使用的有限上限。
///
/// `0` 是旧版本的默认值，因此必须在运行边界再次兜底，不能只依赖 serde
/// 默认值；手工构造或存量配置都要得到相同的安全行为。
pub fn effective_agent_steps(steps: u32) -> u32 {
    if steps == 0 {
        DEFAULT_AGENT_STEPS
    } else {
        steps
    }
}

#[cfg(test)]
mod tests {
    use super::{effective_agent_steps, DEFAULT_AGENT_STEPS};

    #[test]
    fn 零轮数使用有限默认上限() {
        assert_eq!(effective_agent_steps(0), DEFAULT_AGENT_STEPS);
        assert!(DEFAULT_AGENT_STEPS > 0);
    }

    #[test]
    fn 显式轮数保持原值() {
        assert_eq!(effective_agent_steps(7), 7);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandDef {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// 限定执行 agent(缺省=当前)。
    #[serde(default)]
    pub agent: Option<String>,
    /// 模板正文,支持 $ARGUMENTS / $1..$N / @file。
    #[serde(default)]
    pub template: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDef {
    pub name: String,
    pub description: String,
    /// SKILL.md 路径,正文经 skill 工具按需加载。
    pub path: std::path::PathBuf,
}
