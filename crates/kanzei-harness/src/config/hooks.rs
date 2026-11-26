//! 生命周期钩子配置域：确定性本地脚本（pre_tool / post_tool / on_stop），零 Token 消耗。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    PreTool,
    PostTool,
    OnStop,
}

impl HookEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PreTool => "pre_tool",
            Self::PostTool => "post_tool",
            Self::OnStop => "on_stop",
        }
    }
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct HookRule {
    pub event: HookEvent,
    /// 可选工具名匹配器：支持精确名、逗号分隔列表或通配 `*`（如 `edit,write` 或 `*`）。
    /// 对 `on_stop` 事件可省略。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_matcher: Option<String>,
    /// 执行的本地命令（Windows 经 pwsh/cmd，Unix 经 sh -c）。
    pub command: String,
    /// 单条钩子超时秒数（默认 15s，上限 120s）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

impl HookRule {
    pub fn effective_timeout_secs(&self) -> u64 {
        self.timeout_secs.unwrap_or(15).clamp(1, 120)
    }

    pub fn matches_tool(&self, tool_name: &str) -> bool {
        let Some(matcher) = self.tool_matcher.as_deref().map(str::trim) else {
            return true;
        };
        if matcher.is_empty() || matcher == "*" {
            return true;
        }
        matcher.split(',').any(|token| {
            let pat = token.trim();
            if pat.is_empty() {
                return false;
            }
            if pat == "*" {
                return true;
            }
            if let Some(prefix) = pat.strip_suffix('*') {
                return tool_name.starts_with(prefix);
            }
            if let Some(suffix) = pat.strip_prefix('*') {
                return tool_name.ends_with(suffix);
            }
            tool_name.eq_ignore_ascii_case(pat)
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct HooksSection {
    #[serde(default)]
    pub rules: Vec<HookRule>,
}

pub(crate) const HOOKS_KEYS: &[&str] = &["rules"];

pub(crate) const HOOK_RULE_KEYS: &[&str] = &[
    "event",
    "tool_matcher",
    "command",
    "timeout_secs",
    "enabled",
];
