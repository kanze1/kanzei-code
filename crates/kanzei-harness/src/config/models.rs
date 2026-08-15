//! 模型与 provider 域(R-257 B5):ModelRoles/ProviderConfig/ResolvedModel、
//! 上下文窗口回填(known_context_limit)。自 config.rs 原样迁出,零行为变更。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ModelRoles {
    pub primary: Option<String>,
    pub fast: Option<String>,
    /// 思考强度默认档:"off"(默认)| "low" | "medium" | "high"。
    /// 运行时可被桌面端的每进程选择覆盖;未配置时保持 off,行为与既有一致。
    #[serde(default)]
    pub reasoning: Option<String>,
    /// Codex Fast mode:同一模型使用更高消耗的 priority 服务档位。
    #[serde(default)]
    pub codex_fast_mode: Option<bool>,
    /// R-173:阶段编排派发的只读代理(勘察 + 复核)用哪条路由。
    ///
    /// 取值与 primary/fast 同一套解析(角色名或 `provider:model`)。
    /// `None` = 沿用 `fast`,与引入前逐字节一致。
    ///
    /// 单独一个键的理由:勘察/复核是**读密集、上下文短、要看懂代码**的活,
    /// 它该用哪个模型与「主对话用哪个」「机械检索用哪个」都不是同一个问题。
    /// 写死 fast 等于假定勘察是廉价活——用户跑 DeepSeek 时这个假设不成立,
    /// 白白让勘察质量降级。
    #[serde(default)]
    pub scout: Option<String>,
    /// R-236 B3:上下文压缩纪要用哪条路由。取值与 primary/fast 同一套解析。
    ///
    /// `None` = 跟随 **primary**(不是 fast——这是对旧实现的刻意纠偏:纪要质量
    /// 随模型能力显著变化,弱模型摘要在长任务上有 -8pp 的实测消融;主流实现的
    /// 缺省也都是主模型)。想省钱就显式配弱模型,压缩质量闸负责兜底。
    #[serde(default)]
    pub compact: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProviderConfig {
    /// "anthropic" | "openai" | "openai-responses" | "deepseek-responses"
    pub protocol: String,
    pub base_url: String,
    #[serde(default)]
    pub api_key_env: Option<String>,
    /// 直填 API key(个人工具的便利通道,优先于 api_key_env;明文存 toml,自担风险)。
    #[serde(default)]
    pub api_key: Option<String>,
    /// 特殊认证:"codex" = 复用 Codex CLI 登录态,"claude" = 复用 Claude Code 登录态。
    #[serde(default)]
    pub auth: Option<String>,
    /// 上下文窗口(token)。用于界面占用比例显示与(M2)压缩预检。
    #[serde(default)]
    pub context_limit: Option<u64>,
}

impl ProviderConfig {
    /// 返回运行时真正使用的 wire protocol。
    ///
    /// 旧版本把内置 DeepSeek 写成 `openai`。只对“内置名 + 官方地址”的旧配置
    /// 升级到 Responses；自定义兼容服务和显式协议都保持用户原意。
    pub fn effective_protocol<'a>(&'a self, provider_name: &str) -> &'a str {
        if self.protocol == "openai"
            && provider_name.eq_ignore_ascii_case("deepseek")
            && self
                .base_url
                .trim_end_matches('/')
                .eq_ignore_ascii_case("https://api.deepseek.com")
        {
            "deepseek-responses"
        } else {
            &self.protocol
        }
    }
}

/// 解析后的模型指向。
#[derive(Debug, Clone)]
pub struct ResolvedModel {
    pub provider_name: String,
    pub provider: ProviderConfig,
    pub model: String,
}

/// 已知 provider 的上下文窗口。名字优先,其次看 base_url 主机名(用户可能改名)。
/// 认不出来就返回 None——宁可让 UI 显示"未知",也不编一个数字当预算基准。
pub(crate) fn known_context_limit(name: &str, base_url: &str) -> Option<u64> {
    let name = name.to_ascii_lowercase();
    for (needle, limit) in [
        ("anthropic", 200_000u64),
        ("claude", 200_000),
        ("codex", 272_000),
        ("openai", 272_000),
        ("deepseek", 1_000_000),
        ("moonshot", 128_000),
        ("kimi", 128_000),
        ("ollama", 32_000),
    ] {
        if name.contains(needle) || base_url.contains(needle) {
            return Some(limit);
        }
    }
    None
}

/// kanzei.toml [models] / [providers] 节已知键名单(R-220 单源)。
pub(crate) const MODELS_KEYS: &[&str] = &[
    "primary",
    "fast",
    "reasoning",
    "codex_fast_mode",
    "scout",
    "compact",
];
pub(crate) const PROVIDER_KEYS: &[&str] = &[
    "protocol",
    "base_url",
    "api_key_env",
    "api_key",
    "auth",
    "context_limit",
];

/// fill_defaults 无条件回填的内置 provider 名单(R-184 P6 / D-246)。
/// 与 fill_defaults 中的五个 `entry().or_insert()` 保持同步;UI 据此把内置
/// provider 的删除入口换成「内置」标记,避免「删了重开又回来」的误导。
/// 这是只读元数据,不参与配置解析。
pub fn builtin_provider_names() -> &'static [&'static str] {
    &["anthropic", "ollama", "codex", "claude", "deepseek"]
}

/// 内置 provider 的出厂 `context_limit`(取自 fill_defaults 本身,不另立名单——
/// 名单漂移比没有名单更糟,见 D-246)。
///
/// D-288:设置页保存时把**每个** provider 的 context_limit 都写进用户 toml,于是
/// 一次「保存」就把当时的出厂默认冻成了用户配置。deepseek 的出厂值后来从 128k
/// 改成 1M,用户那份 toml 却永远停在 128000——`fill_defaults` 只补 `None`,不会
/// 覆盖已有值,所以内置默认再怎么改都追不上。设置页据此判断「这个数只是出厂默认」,
/// 相同就不落盘,留空即跟随内置。
pub fn builtin_context_limit(name: &str) -> Option<u64> {
    let mut config = crate::config::KanzeiConfig::default();
    config.providers.clear();
    config.fill_defaults();
    config.providers.get(name).and_then(|p| p.context_limit)
}

/// 供 `fill_defaults` 引用(避免 `BTreeMap` 未使用的风险被误报):providers 字段
/// 的真实类型声明在 [`crate::config::KanzeiConfig`],这里仅保留类型别名可寻址。
#[allow(dead_code)]
pub(crate) type ProvidersMap = BTreeMap<String, ProviderConfig>;
