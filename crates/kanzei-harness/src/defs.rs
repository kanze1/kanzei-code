//! Agent / Command / Skill 定义与 Profile 标识。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProfileKind {
    Dev,
    Research,
}

impl std::str::FromStr for ProfileKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "dev" => Ok(ProfileKind::Dev),
            "research" => Ok(ProfileKind::Research),
            other => Err(format!("unknown profile `{other}` (dev|research)")),
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

fn default_model_ref() -> String {
    "primary".into()
}

fn default_steps() -> u32 {
    40
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
