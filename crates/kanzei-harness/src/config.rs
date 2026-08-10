//! kanzei.toml:全局(~/.kanzei/)→ 项目(.kanzei/,从 cwd 向上发现),后者覆盖前者。
//! 配置本身以组件形式进入 harness(贡献权限规则),没有第二条 config→runtime 路径。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::permission::Rule;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct KanzeiConfig {
    #[serde(default)]
    pub models: ModelRoles,
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderConfig>,
    /// "env"(默认)| "off" | 代理地址。
    #[serde(default)]
    pub proxy: Option<String>,
    #[serde(default)]
    pub profile: ProfileSection,
    #[serde(default)]
    pub permissions: PermissionsSection,
    #[serde(default)]
    pub limits: Limits,
    #[serde(default)]
    pub cadence: Cadence,
    /// 向量检索通道(R-164):配置了 provider:model 才启用 dense/hybrid。
    /// 未配置时系统退化为 lexical 通道,功能完整(设计 §5 验收①)。
    #[serde(default)]
    pub embeddings: EmbeddingsSection,
}

/// 运行时上限与阈值。此前全部是散落在各 crate 里的硬编码常量,配置层没有任何入口——
/// 想调一个输出预算就得改代码重编译。
///
/// 每个字段都是 Option:None = 用内置默认(即改造前那个常量值),所以旧配置没有
/// `[limits]` 节时行为逐字节不变(conventions §4 向后兼容);层叠合并也照既有规矩来——
/// 项目层只覆盖它显式写了的那几个键,不会把没写的键打回默认值。
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Limits {
    /// 主对话单次输出上限(tokens)
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// 子代理单次输出上限(tokens)
    #[serde(default)]
    pub subagent_max_tokens: Option<u32>,
    /// 单个子代理的墙钟上限(秒)
    #[serde(default)]
    pub subagent_timeout_secs: Option<u64>,
    /// R-173 汇总/复核屏障的墙钟上界(秒)。None = 由 subagent_timeout_secs 推导
    #[serde(default)]
    pub barrier_timeout_secs: Option<u64>,
    /// 轮内主动压缩的触发线:占上下文窗口的比例
    #[serde(default)]
    pub context_budget_ratio: Option<f64>,
    /// 压缩时末尾逐字保留的比例
    #[serde(default)]
    pub recent_verbatim_ratio: Option<f64>,
    /// 单轮最多派发多少个 task 子代理
    #[serde(default)]
    pub max_tasks_per_turn: Option<usize>,
    /// 单波最多并行多少个工具
    #[serde(default)]
    pub max_parallel_tools: Option<usize>,
    /// 传输层重试次数
    #[serde(default)]
    pub transport_retries: Option<u32>,
    /// 限流重试次数
    #[serde(default)]
    pub rate_limit_retries: Option<u32>,
    /// 流中断后重放本轮的次数上限
    #[serde(default)]
    pub stream_restarts: Option<u32>,
}

impl Limits {
    pub fn max_tokens(&self) -> u32 {
        self.max_tokens.unwrap_or(8192)
    }
    pub fn subagent_max_tokens(&self) -> u32 {
        self.subagent_max_tokens.unwrap_or(4096)
    }
    pub fn subagent_timeout_secs(&self) -> u64 {
        self.subagent_timeout_secs.unwrap_or(900)
    }
    /// R-173 屏障上界:默认由 `subagent_timeout_secs` 推导(×2),不另拍一个数。
    ///
    /// **外层必须永远宽于内层**——每个勘察子代理已被 `subagent_timeout_secs`
    /// 的墙钟包住(见 runner drive 的 task 派发),屏障只是"内层失效时"的兜底。
    /// 把本值配成小于等于子代理上界,会让屏障在子代理**正常工作**时就把它判成
    /// 超时,凭空制造假失败。所以显式配置也按下界夹紧(与本节 `max_parallel_tools`
    /// 的 `.max(1)`、`context_budget_ratio` 的 `.clamp()` 同一口径)。
    pub fn barrier_timeout_secs(&self) -> u64 {
        let inner = self.subagent_timeout_secs();
        self.barrier_timeout_secs
            .unwrap_or(inner.saturating_mul(2))
            .max(inner.saturating_add(1))
    }
    pub fn context_budget_ratio(&self) -> f64 {
        self.context_budget_ratio.unwrap_or(0.7).clamp(0.1, 0.95)
    }
    pub fn recent_verbatim_ratio(&self) -> f64 {
        self.recent_verbatim_ratio.unwrap_or(0.35).clamp(0.05, 0.9)
    }
    pub fn max_tasks_per_turn(&self) -> usize {
        self.max_tasks_per_turn.unwrap_or(8).max(1)
    }
    pub fn max_parallel_tools(&self) -> usize {
        self.max_parallel_tools.unwrap_or(8).max(1)
    }
    pub fn transport_retries(&self) -> u32 {
        self.transport_retries.unwrap_or(2)
    }
    pub fn rate_limit_retries(&self) -> u32 {
        self.rate_limit_retries.unwrap_or(2)
    }
    pub fn stream_restarts(&self) -> u32 {
        self.stream_restarts.unwrap_or(2)
    }
}

/// 向量检索通道配置(R-164)。两个字段都带 serde default:
/// 旧配置没有 `[embeddings]` 节时通道关闭,检索退化为 lexical(设计 §5 验收①)。
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct EmbeddingsSection {
    /// provider 名(providers 表里的键,如 "ollama")。None = 通道关闭。
    #[serde(default)]
    pub provider: Option<String>,
    /// 模型名(如 "nomic-embed-text" / "text-embedding-3-small")。
    #[serde(default)]
    pub model: Option<String>,
}

impl EmbeddingsSection {
    /// 通道是否启用:provider 与 model 都配置了才生效。
    pub fn enabled(&self) -> bool {
        self.provider.as_deref().is_some_and(|p| !p.is_empty())
            && self.model.as_deref().is_some_and(|m| !m.is_empty())
    }
}

/// 验证与提交节奏(R-157):把 conventions §1.4 的节奏参数从提示词硬化成可调配置。
/// 每个字段都带 serde default,旧配置没有 `[cadence]` 节时行为与 §1.4 当前默认
/// 逐项一致(conventions §4 向后兼容);层叠合并照既有规矩——项目层只覆盖显式写的键。
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Cadence {
    /// 全量测试触发档位。默认 EntryClose:条目关闭前一次(发版前 verify.ps1 是
    /// 独立硬门禁,不受本参数影响,见 A-010)。
    #[serde(default)]
    pub full_test: FullTestCadence,
    /// full_test == EveryNBatches 时的批次间隔 n。
    #[serde(default)]
    pub full_test_batches: Option<u32>,
    /// 定向测试:每次提交前必跑(默认)| off。
    #[serde(default)]
    pub targeted_test: TargetedTestCadence,
    /// 提交粒度:每条目一提交 | 每批一提交(默认,多批大条目按批提交)。
    #[serde(default)]
    pub commit: CommitCadence,
    /// push 频率:条目完成后 push(默认)| 每提交后 push | 定期(与 R-143 并轨)。
    #[serde(default)]
    pub push: PushCadence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FullTestCadence {
    /// 条目关闭前一次(§1.4 默认)。
    #[default]
    EntryClose,
    /// 每次提交前全量。
    EveryCommit,
    /// 每 n 批全量一次,间隔见 Cadence::full_test_batches。
    EveryNBatches,
    /// 只发版前跑(verify.ps1 硬门禁,本地开发不跑全量)。
    ReleaseOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetedTestCadence {
    /// 每次提交前必跑(§1.4 默认)。
    #[default]
    EveryCommit,
    /// 关闭定向测试(不推荐;改动面与验证匹配的判断交给模型)。
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitCadence {
    /// 多批大条目每批一提交(§1.4 默认)。
    #[default]
    PerBatch,
    /// 整条目一提交(复杂度小的条目适用)。
    PerEntry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PushCadence {
    /// 条目完成后 push(§1.4 默认)。
    #[default]
    PerEntry,
    /// 每提交后顺手 push。
    PerCommit,
    /// 定期自动 push(与 R-143 自举循环自动 push 并轨)。
    Periodic,
}

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
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProviderConfig {
    /// "anthropic" | "openai" | "openai-responses"
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

#[cfg(test)]
mod context_limit_tests {
    use super::*;

    /// D-173:`entry().or_insert()` 只在整条 provider 缺失时生效,用户配了同名
    /// provider 却漏写 context_limit 时补不上——实测用户的 deepseek/codex/anthropic
    /// 全都没有上限值,UI 占用比例与压缩预检因此全部失去基准。
    #[test]
    fn 用户配置里缺失的上下文上限会被按已知值回填() {
        let mut config = KanzeiConfig::default();
        for (name, base_url) in [
            ("deepseek", "https://api.deepseek.com"),
            ("codex", "https://chatgpt.com/backend-api/codex"),
            ("anthropic", "https://api.anthropic.com"),
        ] {
            config.providers.insert(
                name.into(),
                ProviderConfig {
                    protocol: "openai".into(),
                    base_url: base_url.into(),
                    api_key_env: None,
                    api_key: None,
                    auth: None,
                    context_limit: None,
                },
            );
        }
        // 用户显式写死的值不能被覆盖。
        config.providers.insert(
            "kimi".into(),
            ProviderConfig {
                protocol: "openai".into(),
                base_url: "https://api.moonshot.cn/v1".into(),
                api_key_env: None,
                api_key: None,
                auth: None,
                context_limit: Some(1_000_000),
            },
        );
        config.fill_defaults();

        assert_eq!(config.providers["deepseek"].context_limit, Some(128_000));
        assert_eq!(config.providers["codex"].context_limit, Some(272_000));
        assert_eq!(config.providers["anthropic"].context_limit, Some(200_000));
        assert_eq!(config.providers["kimi"].context_limit, Some(1_000_000));
        // 认不出来的 provider 保持 None——宁可显示"未知",也不编一个预算基准。
        assert_eq!(known_context_limit("mystery", "https://example.test"), None);
    }
}

/// 已知 provider 的上下文窗口。名字优先,其次看 base_url 主机名(用户可能改名)。
/// 认不出来就返回 None——宁可让 UI 显示"未知",也不编一个数字当预算基准。
fn known_context_limit(name: &str, base_url: &str) -> Option<u64> {
    let name = name.to_ascii_lowercase();
    for (needle, limit) in [
        ("anthropic", 200_000u64),
        ("claude", 200_000),
        ("codex", 272_000),
        ("openai", 272_000),
        ("deepseek", 128_000),
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

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ProfileSection {
    pub default: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PermissionsSection {
    #[serde(default)]
    pub rules: Vec<Rule>,
}

/// 解析后的模型指向。
#[derive(Debug, Clone)]
pub struct ResolvedModel {
    pub provider_name: String,
    pub provider: ProviderConfig,
    pub model: String,
}

impl KanzeiConfig {
    /// 全局 + 项目层叠加载。语法/类型错误返回错误(配置错误要炸在启动,不能静默);
    /// 未知字段宽容忽略但产生告警(D-084:schema 向后兼容,新旧二进制可共用同一文件)。
    pub fn load(cwd: &Path) -> anyhow::Result<KanzeiConfig> {
        let (config, warnings) = Self::load_with_warnings(cwd)?;
        for warning in &warnings {
            tracing::warn!("{warning}");
        }
        Ok(config)
    }

    /// 同 load,但把未知字段告警交给调用方展示(CLI stderr / 桌面 kz:status)。
    pub fn load_with_warnings(cwd: &Path) -> anyhow::Result<(KanzeiConfig, Vec<String>)> {
        let mut config = KanzeiConfig::default();
        let mut warnings = Vec::new();
        if let Some(home) = crate::home::kanzei_home() {
            merge_file(&mut config, &home.join("kanzei.toml"), &mut warnings)?;
        }
        if let Some(project) = discover_project_config(cwd) {
            merge_file(&mut config, &project, &mut warnings)?;
        }
        config.fill_defaults();
        Ok((config, warnings))
    }

    /// 找出升级到结构化 bash 资源前遗留的裸命令规则；只读，不修改配置。
    pub fn legacy_bash_rules(&self) -> Vec<&Rule> {
        self.permissions
            .rules
            .iter()
            .filter(|rule| {
                rule.action == "bash"
                    && !serde_json::from_str::<serde_json::Value>(&rule.resource)
                        .ok()
                        .is_some_and(|resource| {
                            resource.get("command").is_some() && resource.get("workdir").is_some()
                        })
            })
            .collect()
    }

    /// 裸 bash 规则中需要降级为逐次询问的规则；显式 `bash/*` 放行另行提示。
    /// deny 规则是仍然生效的护栏,不该被算作"将逐次询问"(D-139)。
    pub fn legacy_bash_rules_needing_downgrade(&self) -> Vec<&Rule> {
        self.legacy_bash_rules()
            .into_iter()
            .filter(|rule| {
                !is_wildcard_resource(&rule.resource)
                    && rule.effect != crate::permission::Effect::Deny
            })
            .collect()
    }

    /// 显式 `bash/* = allow` 保持全量放行语义，启动时必须明确告知用户。
    pub fn explicit_bash_wildcard_allows(&self) -> Vec<&Rule> {
        self.permissions
            .rules
            .iter()
            .filter(|rule| {
                rule.action == "bash"
                    && is_wildcard_resource(&rule.resource)
                    && rule.effect == crate::permission::Effect::Allow
            })
            .collect()
    }

    /// 启动告警(D-139):文案必须由**实际评估结果**推导,而不是按规则形态猜。
    ///
    /// 原实现按规则形态分别计数各说各话:legacy 规则与显式 `bash/*` 并存时
    /// last-match-wins 让一切直接放行,告警却照样说"将逐次询问"——在安全边界上
    /// 给出错误告知。现在先用代表性命令跑一遍 Ruleset::evaluate,以真实判定为准。
    pub fn bash_permission_warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        let mut ruleset = crate::permission::Ruleset::default();
        for rule in &self.permissions.rules {
            ruleset.push(rule.clone());
        }
        // 代表性命令:一条普通只读命令即可探明"默认会不会问"。
        let probe = serde_json::json!({ "command": "git status", "workdir": "." }).to_string();
        let effective = ruleset.evaluate("bash", &probe);

        let legacy_count = self.legacy_bash_rules_needing_downgrade().len();
        let wildcard_count = self.explicit_bash_wildcard_allows().len();

        if effective == crate::permission::Effect::Allow {
            // 无论有多少条 legacy 规则,实际结果就是全量放行——必须如实说。
            warnings.push(format!(
                "检测到 bash 权限最终判定为全量放行(yolo){}；不会再逐次询问，请确认这是有意设置。",
                if wildcard_count > 0 {
                    format!("，来自 {wildcard_count} 条显式 bash/* 放行规则")
                } else {
                    String::new()
                }
            ));
            if legacy_count > 0 {
                warnings.push(format!(
                    "另有 {legacy_count} 条旧 bash 规则被上述放行覆盖(last-match-wins)，实际不生效。"
                ));
            }
            return warnings;
        }
        if legacy_count > 0 {
            warnings.push(format!(
                "检测到 {legacy_count} 条旧 bash 权限规则；将逐次询问，请重新选择精确作用域。"
            ));
        }
        warnings
    }

    pub fn fill_defaults(&mut self) {
        self.providers
            .entry("anthropic".into())
            .or_insert(ProviderConfig {
                protocol: "anthropic".into(),
                base_url: "https://api.anthropic.com".into(),
                api_key_env: Some("ANTHROPIC_API_KEY".into()),
                api_key: None,
                auth: None,
                context_limit: Some(200_000),
            });
        self.providers
            .entry("ollama".into())
            .or_insert(ProviderConfig {
                protocol: "openai".into(),
                base_url: "http://127.0.0.1:11434/v1".into(),
                api_key_env: None,
                api_key: None,
                auth: None,
                context_limit: Some(32_000),
            });
        // Codex 订阅通道:复用 Codex CLI 登录态,零配置可用。
        self.providers
            .entry("codex".into())
            .or_insert(ProviderConfig {
                protocol: "openai-responses".into(),
                base_url: "https://chatgpt.com/backend-api/codex".into(),
                api_key_env: None,
                api_key: None,
                auth: Some("codex".into()),
                context_limit: Some(272_000),
            });
        // Claude Code 订阅通道:复用 Claude Code 登录态,零配置可用。
        self.providers
            .entry("claude".into())
            .or_insert(ProviderConfig {
                protocol: "anthropic".into(),
                base_url: "https://api.anthropic.com".into(),
                api_key_env: None,
                api_key: None,
                auth: Some("claude".into()),
                context_limit: Some(200_000),
            });
        // DeepSeek 直连(OpenAI 兼容)。
        self.providers
            .entry("deepseek".into())
            .or_insert(ProviderConfig {
                protocol: "openai".into(),
                base_url: "https://api.deepseek.com".into(),
                api_key_env: Some("DEEPSEEK_API_KEY".into()),
                api_key: None,
                auth: None,
                context_limit: Some(128_000),
            });
        // context_limit 逐字段兜底(D-173)。
        //
        // 上面全是 `entry().or_insert()`:用户只要在 kanzei.toml 里写了同名 provider,
        // 整条默认就不再生效,**单个缺失字段也补不上**。实测后果是用户配置里的
        // deepseek/codex/anthropic 全都没有 context_limit,于是 UI 的占用比例、
        // 运行器的压缩预检统统失去基准,只能等 provider 报 overflow 才被动补救。
        // 这里按 provider 名与 base_url 主机名回填一个已知值;用户显式写了就不动。
        for (name, provider) in self.providers.iter_mut() {
            if provider.context_limit.is_some() {
                continue;
            }
            let host = provider.base_url.to_ascii_lowercase();
            provider.context_limit = known_context_limit(name, &host);
        }
        if self.models.primary.is_none() {
            self.models.primary = Some("codex:gpt-5.6-luna".into());
        }
        if self.models.fast.is_none() {
            self.models.fast = Some("ollama:qwen3.5:4b".into());
        }
        if self.models.codex_fast_mode.is_none()
            && self.models.primary.as_deref() == Some("codex:gpt-5.6-luna")
        {
            self.models.codex_fast_mode = Some(true);
        }
    }

    /// "primary"/"fast"(角色)或 "provider:model"(直指)→ ResolvedModel。
    /// Codex Fast mode 的唯一判据:开关打开 **且** 本次真正解析到的供应商是 codex。
    /// 判据只有这一份——此前 11 个 RunnerConfig 构造点里 9 个硬写 None,于是"用 codex
    /// 就该生效"在子代理、压缩纪要、记忆整理、文件标注这些路径上全部失效(用户实测提出)。
    /// 任何新增的构造点都必须调它,不要再就地抄条件。
    pub fn service_tier_for(&self, resolved: &ResolvedModel) -> Option<String> {
        (self.models.codex_fast_mode.unwrap_or(false)
            && resolved.provider.auth.as_deref() == Some("codex"))
        .then(|| "priority".to_string())
    }

    pub fn resolve_model(&self, reference: &str) -> anyhow::Result<ResolvedModel> {
        let spec = match reference {
            "primary" => self.models.primary.as_deref().unwrap_or_default(),
            "fast" => self.models.fast.as_deref().unwrap_or_default(),
            direct => direct,
        };
        let (provider_name, model) = spec.split_once(':').ok_or_else(|| {
            anyhow::anyhow!(
                "model reference `{spec}` must be `provider:model` (from `{reference}`)"
            )
        })?;
        let provider = self.providers.get(provider_name).ok_or_else(|| {
            anyhow::anyhow!(
                "unknown provider `{provider_name}`; configured: {}",
                self.providers
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
        Ok(ResolvedModel {
            provider_name: provider_name.to_string(),
            provider: provider.clone(),
            model: model.to_string(),
        })
    }

    pub fn default_profile(&self) -> crate::defs::ProfileKind {
        self.profile
            .default
            .as_deref()
            .and_then(|s| s.parse().ok())
            .unwrap_or(crate::defs::ProfileKind::Dev)
    }
}

fn merge_file(
    config: &mut KanzeiConfig,
    path: &Path,
    warnings: &mut Vec<String>,
) -> anyhow::Result<()> {
    if !path.is_file() {
        return Ok(());
    }
    let text = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", path.display()))?;
    let layer: KanzeiConfig = toml::from_str(&text)
        .map_err(|e| anyhow::anyhow!("invalid config {}: {e}", path.display()))?;
    // 未知键宽容忽略但必须可见:拼错的键静默失效比报错更难排查。
    if let Ok(raw) = toml::from_str::<toml::Value>(&text) {
        for key in unknown_keys(&raw) {
            warnings.push(format!(
                "{}: 未知配置项 `{key}` 已忽略(可能是拼写错误,或来自更新版本的 kanzei)",
                path.display()
            ));
        }
    }
    merge(config, layer);
    Ok(())
}

/// `*` 通配资源判定:全仓统一按 trim 后比较,避免两处判定不一致(D-139)。
fn is_wildcard_resource(resource: &str) -> bool {
    resource.trim() == "*"
}

/// 列出 schema 未识别的键路径。schema 变更时同步维护;
/// `unknown_keys_schema_matches_struct` 测试守护两者不漂移。
fn unknown_keys(value: &toml::Value) -> Vec<String> {
    fn check(table: &toml::Value, path: &str, known: &[&str], out: &mut Vec<String>) {
        let Some(table) = table.as_table() else {
            return;
        };
        for key in table.keys() {
            if !known.contains(&key.as_str()) {
                out.push(if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                });
            }
        }
    }
    let mut out = Vec::new();
    check(
        value,
        "",
        &[
            "models",
            "providers",
            "proxy",
            "profile",
            "permissions",
            "limits",
            "cadence",
            "embeddings",
        ],
        &mut out,
    );
    if let Some(models) = value.get("models") {
        check(
            models,
            "models",
            &["primary", "fast", "reasoning", "codex_fast_mode"],
            &mut out,
        );
    }
    if let Some(embeddings) = value.get("embeddings") {
        check(embeddings, "embeddings", &["provider", "model"], &mut out);
    }
    if let Some(limits) = value.get("limits") {
        check(
            limits,
            "limits",
            &[
                "max_tokens",
                "subagent_max_tokens",
                "subagent_timeout_secs",
                "context_budget_ratio",
                "recent_verbatim_ratio",
                "max_tasks_per_turn",
                "max_parallel_tools",
                "transport_retries",
                "rate_limit_retries",
                "stream_restarts",
            ],
            &mut out,
        );
    }
    if let Some(providers) = value.get("providers").and_then(|p| p.as_table()) {
        for (name, provider) in providers {
            check(
                provider,
                &format!("providers.{name}"),
                &[
                    "protocol",
                    "base_url",
                    "api_key_env",
                    "api_key",
                    "auth",
                    "context_limit",
                ],
                &mut out,
            );
        }
    }
    if let Some(profile) = value.get("profile") {
        check(profile, "profile", &["default"], &mut out);
    }
    if let Some(cadence) = value.get("cadence") {
        check(
            cadence,
            "cadence",
            &[
                "full_test",
                "full_test_batches",
                "targeted_test",
                "commit",
                "push",
            ],
            &mut out,
        );
    }
    if let Some(permissions) = value.get("permissions") {
        check(permissions, "permissions", &["rules"], &mut out);
        if let Some(rules) = permissions.get("rules").and_then(|r| r.as_array()) {
            for (index, rule) in rules.iter().enumerate() {
                check(
                    rule,
                    &format!("permissions.rules[{index}]"),
                    &["action", "resource", "effect"],
                    &mut out,
                );
            }
        }
    }
    out
}

/// 标量覆盖、map 合并、规则追加(后层排后 → last-match-wins 自然让后层优先)。
fn merge(base: &mut KanzeiConfig, layer: KanzeiConfig) {
    if layer.models.primary.is_some() {
        base.models.primary = layer.models.primary;
    }
    if layer.models.fast.is_some() {
        base.models.fast = layer.models.fast;
    }
    // reasoning 曾被漏掉:项目级设的思考强度会被静默忽略,而 primary/fast 却生效,
    // 表现为"同一份配置里有的键管用有的不管用"——最难查的那种。
    if layer.models.reasoning.is_some() {
        base.models.reasoning = layer.models.reasoning;
    }
    if layer.models.codex_fast_mode.is_some() {
        base.models.codex_fast_mode = layer.models.codex_fast_mode;
    }
    base.providers.extend(layer.providers);
    if layer.proxy.is_some() {
        base.proxy = layer.proxy;
    }
    if layer.profile.default.is_some() {
        base.profile.default = layer.profile.default;
    }
    base.permissions.rules.extend(layer.permissions.rules);
    // [limits] 逐字段覆盖:项目层只写了哪几个键就只覆盖哪几个,没写的保持全局层的值。
    // 整节替换会让"项目里只调一个 max_tokens"把其余全部打回默认——正是 reasoning
    // 那次漏合并留下的教训。
    macro_rules! overlay {
        ($($field:ident),+ $(,)?) => {
            $(if layer.limits.$field.is_some() { base.limits.$field = layer.limits.$field; })+
        };
    }
    overlay!(
        max_tokens,
        subagent_max_tokens,
        subagent_timeout_secs,
        context_budget_ratio,
        recent_verbatim_ratio,
        max_tasks_per_turn,
        max_parallel_tools,
        transport_retries,
        rate_limit_retries,
        stream_restarts,
    );
    // [embeddings] 逐字段覆盖(与 [limits] 同规:项目层只覆盖写了的那几个键)。
    if layer.embeddings.provider.is_some() {
        base.embeddings.provider = layer.embeddings.provider;
    }
    if layer.embeddings.model.is_some() {
        base.embeddings.model = layer.embeddings.model;
    }
}

/// "总是允许"的持久化:向项目配置追加 allow 规则(后来的规则 last-match-wins)。
/// 文本级追加(D-083):toml_edit 保留注释、排版与未知字段,不做整文件 round-trip。
pub fn append_allow_rule(
    project_root: &Path,
    action: &str,
    resource: &str,
) -> anyhow::Result<PathBuf> {
    let path = project_root.join(".kanzei").join("kanzei.toml");
    let text = if path.is_file() {
        std::fs::read_to_string(&path)?
    } else {
        String::new()
    };
    // 语义预检:类型错误在这里明确报出,而不是把规则追进一个坏文件。
    toml::from_str::<KanzeiConfig>(&text)
        .map_err(|e| anyhow::anyhow!("invalid {}: {e}", path.display()))?;
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid {}: {e}", path.display()))?;
    let permissions = doc.entry("permissions").or_insert(toml_edit::table());
    let Some(permissions) = permissions.as_table_mut() else {
        anyhow::bail!("{}: `permissions` 不是表,无法追加规则", path.display());
    };
    permissions.set_implicit(true);
    let rules = permissions
        .entry("rules")
        .or_insert(toml_edit::Item::ArrayOfTables(
            toml_edit::ArrayOfTables::new(),
        ));
    let Some(rules) = rules.as_array_of_tables_mut() else {
        anyhow::bail!(
            "{}: `permissions.rules` 不是数组表,无法追加规则",
            path.display()
        );
    };
    let mut rule = toml_edit::Table::new();
    rule.insert("action", toml_edit::value(action));
    rule.insert("resource", toml_edit::value(resource));
    rule.insert("effect", toml_edit::value("allow"));
    rules.push(rule);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, doc.to_string())?;
    Ok(path)
}

/// "总是允许"时保留具体资源，避免把一个命令的授权扩大为首词通配。
/// bash 的 shell/解释器语义无法靠首词推断安全边界；调用方仍可对
/// 用户明确配置的整体 `*` 使用 yolo 语义。
pub fn generalize_resource(action: &str, resource: &str) -> String {
    let _ = action;
    resource.to_string()
}

/// 从 cwd 向上找 `.kanzei/kanzei.toml`。
pub fn discover_project_config(cwd: &Path) -> Option<PathBuf> {
    discover_project_root(cwd).map(|root| root.join(".kanzei").join("kanzei.toml"))
}

/// 项目根 = 向上**最近**的含 `.kanzei/` 或 `.git/` 的目录;都没有则 cwd 本身。
///
/// 两条约束都是踩出来的,别再退回去:
/// ① `.kanzei` 不许无视距离压过 `.git`。原实现撞到任何 `.kanzei` 就立即返回,`.git`
///    只记 fallback 且要等循环走完才用,于是 `~/Documents/某仓库`(有 .git、没 .kanzei)
///    会一路走到 HOME,仓库自己的 `.git` 被丢掉。
/// ② HOME 自己的 `.kanzei` 不算项目标记——它是**全局**配置根(kanzei.toml、memory、
///    app.json),必然存在,于是成了 HOME 下所有无标记目录的磁铁。实测后果:`~/.kanzei`
///    里已经躺着 `project/` 与 `state.db` 这类只该出现在项目里的产物。
///    HOME 的 `.git`(dotfiles 仓库)仍然算标记,那是货真价实的仓库。
pub fn discover_project_root(cwd: &Path) -> Option<PathBuf> {
    discover_project_root_with_home(cwd, dirs::home_dir().as_deref())
}

/// 目录比较用的形态:剥 Windows 扩展长度前缀、统一分隔符、去尾分隔符,Windows 上再小写。
///
/// 裸 `==` 比较不够(D-194):`dirs::home_dir()` 给 `C:\Users\kanzei`,而走上来的祖先
/// 可能是 `c:\users\kanzei`(shell 里键入的大小写)或 `\\?\C:\Users\kanzei`(canonicalize
/// 的产物)——任一形态对不上,HOME 判断就静默失效,`~/.kanzei` 立刻变回项目根磁铁。
/// 同一个坑 kanzei-core 的 `session_identity` 已经踩过一次(同一项目裂成两条会话线)。
/// 这里是纯比较、不进哈希,所以可以比那边更狠:分隔符也一并统一。
fn dir_key(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let stripped = raw
        .strip_prefix(r"\\?\UNC\")
        .map(|rest| format!(r"\\{rest}"))
        .or_else(|| raw.strip_prefix(r"\\?\").map(str::to_string))
        .unwrap_or_else(|| raw.to_string());
    // 分隔符与大小写只在 Windows 上等价;Linux 下 `a\b` 是个合法文件名、`C:` 与 `c:`
    // 是两个目录,归一过头会把不同路径判成同一个。
    #[cfg(windows)]
    let key = stripped.replace('/', "\\").to_lowercase();
    #[cfg(not(windows))]
    let key = stripped;
    key.trim_end_matches(['\\', '/']).to_string()
}

/// 解析出的项目根是不是 HOME 本身。
///
/// D-189 让 HOME 的 `.kanzei` 不再把子目录吸上去,但在 HOME 里**直接**开跑这条路还通着:
/// 一路向上找不到任何标记时兜底返回 cwd,而 cwd 就是 HOME。此时项目级产物(state.db、
/// project/、memory/)会落进 `~/.kanzei`——那是全局配置根,两边数据就此混在一起
/// (D-186 的残留正是这么来的)。调用方拿这个判据在开跑前拦下来。
pub fn is_home_root(root: &Path) -> bool {
    dirs::home_dir().is_some_and(|home| dir_key(&home) == dir_key(root))
}

fn discover_project_root_with_home(cwd: &Path, home: Option<&Path>) -> Option<PathBuf> {
    let home_key = home.map(dir_key);
    let mut dir = Some(cwd);
    while let Some(d) = dir {
        let is_home = home_key.as_ref().is_some_and(|h| *h == dir_key(d));
        if (!is_home && d.join(".kanzei").is_dir()) || d.join(".git").is_dir() {
            return Some(d.to_path_buf());
        }
        dir = d.parent();
    }
    Some(cwd.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_model_resolution() {
        let mut c = KanzeiConfig::default();
        c.fill_defaults();
        let m = c.resolve_model("primary").unwrap();
        assert_eq!(m.provider_name, "codex");
        assert_eq!(m.model, "gpt-5.6-luna");
        // 默认 primary 是 Luna 时,Codex Fast mode 默认开启(R-158)。
        assert_eq!(c.models.codex_fast_mode, Some(true));
        let claude = c.resolve_model("claude:claude-sonnet-4-6").unwrap();
        assert_eq!(claude.provider.protocol, "anthropic");
        assert_eq!(claude.provider.auth.as_deref(), Some("claude"));
        let m = c.resolve_model("fast").unwrap();
        assert_eq!(m.provider_name, "ollama");
        let m = c.resolve_model("ollama:llama3.3").unwrap();
        assert_eq!(m.model, "llama3.3");
        assert!(c.resolve_model("nope").is_err());
    }

    #[test]
    fn embeddings_缺节关闭_配置后启用_旧配置行为不变() {
        // 旧配置没有 [embeddings] → 通道关闭,检索退化为 lexical(验收①精神)。
        let empty: KanzeiConfig = toml::from_str("").unwrap();
        assert!(!empty.embeddings.enabled());

        // 只配 provider 不配 model → 仍关闭(缺一半不算启用)。
        let half: KanzeiConfig = toml::from_str("[embeddings]\nprovider = \"ollama\"\n").unwrap();
        assert!(!half.embeddings.enabled());

        // 配全 → 启用。
        let full: KanzeiConfig =
            toml::from_str("[embeddings]\nprovider = \"ollama\"\nmodel = \"nomic-embed-text\"\n")
                .unwrap();
        assert!(full.embeddings.enabled());
        assert_eq!(full.embeddings.provider.as_deref(), Some("ollama"));
        assert_eq!(full.embeddings.model.as_deref(), Some("nomic-embed-text"));
    }

    #[test]
    fn limits_缺节等于内置默认_项目层只覆盖它写了的那几个键() {
        // 旧配置没有 [limits] 时必须逐值等于改造前的硬编码常量。
        let empty: KanzeiConfig = toml::from_str("").unwrap();
        assert_eq!(empty.limits.max_tokens(), 8192);
        assert_eq!(empty.limits.subagent_timeout_secs(), 900);
        assert_eq!(empty.limits.context_budget_ratio(), 0.7);
        assert_eq!(empty.limits.max_tasks_per_turn(), 8);

        // 项目层只写一个键,不能把全局层其余的键打回默认(reasoning 那次漏合并的教训)。
        let mut base: KanzeiConfig =
            toml::from_str("[limits]\nmax_tokens = 4096\nsubagent_timeout_secs = 300\n").unwrap();
        let layer: KanzeiConfig = toml::from_str("[limits]\nmax_tokens = 16384\n").unwrap();
        merge(&mut base, layer);
        assert_eq!(base.limits.max_tokens(), 16384, "项目层写了的键要覆盖");
        assert_eq!(
            base.limits.subagent_timeout_secs(),
            300,
            "没写的键必须保住全局层的值"
        );

        // 离谱取值被夹住,不至于把运行时配崩。
        let wild: KanzeiConfig =
            toml::from_str("[limits]\ncontext_budget_ratio = 9.0\nmax_tasks_per_turn = 0\n")
                .unwrap();
        assert_eq!(wild.limits.context_budget_ratio(), 0.95);
        assert_eq!(wild.limits.max_tasks_per_turn(), 1);
    }

    /// R-173:屏障上界由子代理上界推导,且**永远宽于内层**。
    /// 配窄了会在子代理正常工作时误判超时,所以下界被夹住。
    #[test]
    fn 屏障上界由子代理上界推导且永远宽于内层() {
        let empty: KanzeiConfig = toml::from_str("").unwrap();
        assert_eq!(empty.limits.subagent_timeout_secs(), 900);
        assert_eq!(empty.limits.barrier_timeout_secs(), 1800, "默认 = 内层 ×2");

        // 跟着内层走:调小子代理上界,屏障默认值同步收窄,不用两处各配一遍。
        let derived: KanzeiConfig =
            toml::from_str("[limits]\nsubagent_timeout_secs = 60\n").unwrap();
        assert_eq!(derived.limits.barrier_timeout_secs(), 120);

        // 显式配置生效。
        let explicit: KanzeiConfig =
            toml::from_str("[limits]\nsubagent_timeout_secs = 60\nbarrier_timeout_secs = 300\n")
                .unwrap();
        assert_eq!(explicit.limits.barrier_timeout_secs(), 300);

        // 配得比内层还窄 → 夹到内层之上,屏障不会在子代理仍合法运行时误伤。
        let narrow: KanzeiConfig =
            toml::from_str("[limits]\nsubagent_timeout_secs = 900\nbarrier_timeout_secs = 10\n")
                .unwrap();
        assert!(
            narrow.limits.barrier_timeout_secs() > narrow.limits.subagent_timeout_secs(),
            "屏障上界必须严格宽于子代理上界"
        );
    }

    #[test]
    fn fast_mode_只看开关与本次解析到的供应商() {
        // R-158 验收③的判据此前只是两处就地抄写的表达式,一个测试都没有,
        // 于是另外 9 个构造点硬写 None 也没人发现(用户实测提出)。
        let mut c = KanzeiConfig::default();
        c.fill_defaults();
        c.providers.insert(
            "mock".into(),
            ProviderConfig {
                protocol: "openai".into(),
                base_url: "x".into(),
                api_key_env: None,
                api_key: None,
                auth: None,
                context_limit: None,
            },
        );
        let codex = c.resolve_model("codex:gpt-5.6-luna").unwrap();
        let other = c.resolve_model("mock:whatever").unwrap();

        c.models.codex_fast_mode = Some(true);
        assert_eq!(c.service_tier_for(&codex).as_deref(), Some("priority"));
        assert_eq!(c.service_tier_for(&other), None, "非 codex 供应商一律不发");

        c.models.codex_fast_mode = Some(false);
        assert_eq!(c.service_tier_for(&codex), None, "开关关掉就不发");
        c.models.codex_fast_mode = None;
        assert_eq!(c.service_tier_for(&codex), None, "缺字段按关闭处理");
    }

    #[test]
    fn bash_always_allow_keeps_exact_command() {
        assert_eq!(generalize_resource("bash", "git status"), "git status");
        assert_eq!(generalize_resource("write", "notes.md"), "notes.md");
    }

    #[test]
    fn bash_告警必须与实际评估一致() {
        // D-139:原实现按规则形态分别计数,混合形态下说假话。
        let parse = |text: &str| -> KanzeiConfig { toml::from_str(text).unwrap() };

        // ① 只有 legacy 规则:确实会逐次询问。
        let only_legacy = parse(
            "[[permissions.rules]]\naction = \"bash\"\nresource = \"git status\"\neffect = \"allow\"\n",
        );
        let w = only_legacy.bash_permission_warnings();
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("逐次询问"), "{w:?}");

        // ② legacy + 显式 bash/* allow 并存:last-match-wins 实际全量放行,
        //    绝不能再说"将逐次询问"——这正是原实现的谎言。
        let mixed = parse(
            "[[permissions.rules]]\naction = \"bash\"\nresource = \"git status\"\neffect = \"allow\"\n\
             [[permissions.rules]]\naction = \"bash\"\nresource = \"*\"\neffect = \"allow\"\n",
        );
        let w = mixed.bash_permission_warnings();
        assert!(w[0].contains("全量放行"), "混合形态必须如实说放行: {w:?}");
        assert!(
            !w.iter().any(|line| line.contains("将逐次询问")),
            "混合形态不得再说会询问: {w:?}"
        );
        assert!(w[1].contains("不生效"), "应说明 legacy 规则被覆盖: {w:?}");

        // ③ deny 是仍然生效的护栏,不该被算进"将逐次询问"的计数。
        let with_deny = parse(
            "[[permissions.rules]]\naction = \"bash\"\nresource = \"rm *\"\neffect = \"deny\"\n",
        );
        assert!(
            with_deny.bash_permission_warnings().is_empty(),
            "deny 护栏不该触发降级告警: {:?}",
            with_deny.bash_permission_warnings()
        );

        // ④ 空白包裹的通配与裸通配判定必须一致。
        let padded = parse(
            "[[permissions.rules]]\naction = \"bash\"\nresource = \" * \"\neffect = \"allow\"\n",
        );
        assert_eq!(padded.explicit_bash_wildcard_allows().len(), 1);
    }

    #[test]
    fn unknown_fields_are_tolerated_and_reported() {
        // D-084:新版本写入的配置节不能炸掉旧二进制;未知键忽略但可见。
        let text = r#"
[models]
primary = "anthropic:claude-sonnet-5"
new_feature_field = "from-a-newer-kanzei"

[future_section]
whatever = 1

[providers.kimi]
protocol = "openai"
base_url = "https://api.moonshot.cn/v1"
typo_fielt = true
"#;
        let config: KanzeiConfig = toml::from_str(text).unwrap();
        assert_eq!(
            config.models.primary.as_deref(),
            Some("anthropic:claude-sonnet-5")
        );
        assert!(config.providers.contains_key("kimi"));
        let raw: toml::Value = toml::from_str(text).unwrap();
        let mut unknown = unknown_keys(&raw);
        unknown.sort();
        assert_eq!(
            unknown,
            vec![
                "future_section",
                "models.new_feature_field",
                "providers.kimi.typo_fielt"
            ]
        );
    }

    #[test]
    fn unknown_keys_schema_matches_struct() {
        // 守护 unknown_keys 的手写清单与结构体不漂移:全量序列化后必须零告警。
        let mut config = KanzeiConfig::default();
        config.fill_defaults();
        config.models.reasoning = Some("high".into());
        config.proxy = Some("http://127.0.0.1:7890".into());
        config.profile.default = Some("dev".into());
        config.permissions.rules.push(Rule {
            action: "bash".into(),
            resource: "git status".into(),
            effect: crate::permission::Effect::Allow,
        });
        let raw: toml::Value = toml::from_str(&toml::to_string_pretty(&config).unwrap()).unwrap();
        assert_eq!(unknown_keys(&raw), Vec::<String>::new());
    }

    #[test]
    fn append_allow_rule_preserves_comments_and_unknown_fields() {
        // D-083:追加规则不得抹掉用户手写的注释、排版与未知字段。
        let root = std::env::temp_dir().join(format!(
            "kanzei-d083-config-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".kanzei")).unwrap();
        let path = root.join(".kanzei").join("kanzei.toml");
        std::fs::write(
            &path,
            "# 我的配置,别动我的注释\n[models]\nprimary = \"anthropic:claude-sonnet-5\" # 行尾注释\n\n[future_section]\nkeep_me = true\n",
        )
        .unwrap();
        append_allow_rule(&root, "bash", "git status").unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        for expected in [
            "# 我的配置,别动我的注释",
            "# 行尾注释",
            "[future_section]",
            "keep_me = true",
        ] {
            assert!(
                saved.contains(expected),
                "missing preserved text: {expected}\n---\n{saved}"
            );
        }
        let config: KanzeiConfig = toml::from_str(&saved).unwrap();
        assert_eq!(config.permissions.rules.len(), 1);
        assert_eq!(config.permissions.rules[0].resource, "git status");
        std::fs::remove_dir_all(root).unwrap();
    }

    /// 造一棵 `<tmp>/home/{.kanzei, projects/repo/{.git, src}}`,用来验证项目根解析。
    fn project_root_fixture(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kanzei-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("home").join(".kanzei")).unwrap();
        std::fs::create_dir_all(root.join("home").join("projects").join("repo").join(".git"))
            .unwrap();
        std::fs::create_dir_all(root.join("home").join("projects").join("repo").join("src"))
            .unwrap();
        root
    }

    #[test]
    fn nearest_git_wins_over_a_farther_kanzei() {
        // 更近的 `.git` 必须赢过更远的 `.kanzei`,否则仓库自己的根被丢掉、
        // 一路解析到 HOME(注释一直写的是"最近",实现却不是)。
        let root = project_root_fixture("project-root-nearest");
        let home = root.join("home");
        let repo = home.join("projects").join("repo");
        assert_eq!(
            discover_project_root_with_home(&repo.join("src"), Some(&home)),
            Some(repo.clone())
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn home_global_config_dir_is_not_a_project_marker() {
        // `~/.kanzei` 是全局配置根、必然存在。它若算项目标记,HOME 下所有无标记目录
        // 都会解析到 HOME,项目级产物(project/、state.db)就会漏进全局目录。
        let root = project_root_fixture("project-root-home");
        let home = root.join("home");
        let plain = home.join("scratch");
        std::fs::create_dir_all(&plain).unwrap();
        // 一对前后对照:同一棵目录树,只有"这个目录是不是 HOME"这一个变量。
        // (不断言具体落点——临时目录本身就在真实 HOME 之下,再往上必然还有标记。)
        assert_eq!(
            discover_project_root_with_home(&plain, None),
            Some(home.clone()),
            "不认 HOME 时,~/.kanzei 就是把 HOME 变成项目根的那块磁铁"
        );
        assert_ne!(
            discover_project_root_with_home(&plain, Some(&home)),
            Some(home.clone()),
            "HOME 的 .kanzei 是全局配置根,不该被当成项目标记"
        );
        // 但 HOME 之外的 `.kanzei` 仍然是标记。
        let marked = home.join("projects");
        std::fs::create_dir_all(marked.join(".kanzei")).unwrap();
        assert_eq!(
            discover_project_root_with_home(&marked.join("nested"), Some(&home)),
            Some(marked)
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    /// D-194:HOME 判断必须扛得住同一目录的不同写法,否则 D-189 的排除等于没做。
    #[test]
    fn home_marker_exclusion_survives_path_form_differences() {
        let root = project_root_fixture("project-root-home-forms");
        let home = root.join("home");
        let plain = home.join("scratch");
        std::fs::create_dir_all(&plain).unwrap();

        // 这些写法在 Windows 上指的是同一个目录:扩展长度前缀(canonicalize 的产物)、
        // 正斜杠、末尾分隔符、不同大小写(shell 里键入的)。任何一种没被归一,
        // `~/.kanzei` 就重新变回项目根磁铁。
        let mut variants = vec![PathBuf::from(format!(r"\\?\{}", home.display()))];
        #[cfg(windows)]
        variants.extend([
            PathBuf::from(home.display().to_string().replace('\\', "/")),
            PathBuf::from(format!("{}\\", home.display())),
            PathBuf::from(home.display().to_string().to_lowercase()),
        ]);
        for variant in variants {
            assert_ne!(
                discover_project_root_with_home(&plain, Some(&variant)),
                Some(home.clone()),
                "HOME 写成 {} 时排除失效",
                variant.display()
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    /// D-194:真实 HOME 必须被 `is_home_root` 认出来——CLI 靠它在开跑前拦下
    /// "项目级产物落进全局配置根"。
    #[test]
    fn is_home_root_recognizes_real_home_in_any_form() {
        let Some(home) = dirs::home_dir() else {
            return; // 无 HOME 的环境跳过,不是被测行为。
        };
        assert!(is_home_root(&home));
        #[cfg(windows)]
        {
            assert!(is_home_root(&PathBuf::from(format!(
                "{}\\",
                home.display()
            ))));
            assert!(is_home_root(&PathBuf::from(
                home.display().to_string().replace('\\', "/")
            )));
            assert!(is_home_root(&PathBuf::from(
                home.display().to_string().to_uppercase()
            )));
        }
        assert!(!is_home_root(&home.join("projects")));
    }

    #[test]
    fn append_allow_rule_preserves_structured_bash_scope() {
        let root = std::env::temp_dir().join(format!(
            "kanzei-d051-config-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let resource =
            r#"{"command":"git status > .kanzei/project/requirements.md","workdir":"C:/project"}"#;
        let path = append_allow_rule(&root, "bash", resource).unwrap();
        let saved: KanzeiConfig = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(saved.permissions.rules.len(), 1);
        assert_eq!(saved.permissions.rules[0].action, "bash");
        assert_eq!(saved.permissions.rules[0].resource, resource);
        assert_eq!(
            saved.permissions.rules[0].effect,
            crate::permission::Effect::Allow
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn legacy_bash_rules_are_detected_without_rewriting_config() {
        let config: KanzeiConfig = toml::from_str(
            r#"
[[permissions.rules]]
 action = "bash"
 resource = "git status"
 effect = "allow"
[[permissions.rules]]
 action = "bash"
 resource = "*"
 effect = "allow"
[[permissions.rules]]
 action = "bash"
 resource = '{"command":"git status","workdir":"C:/project"}'
 effect = "allow"
[[permissions.rules]]
 action = "write"
 resource = "notes.md"
 effect = "allow"
"#,
        )
        .unwrap();
        let legacy = config.legacy_bash_rules();
        assert_eq!(legacy.len(), 2);
        assert_eq!(legacy[0].resource, "git status");
        assert_eq!(config.legacy_bash_rules_needing_downgrade().len(), 1);
        assert_eq!(config.explicit_bash_wildcard_allows().len(), 1);
        // D-139:本夹具是混合形态(legacy + 显式 bash/* allow),last-match-wins 的
        // 真实行为是全量放行。旧断言同时要求"将逐次询问"与"将全量放行",把互相矛盾的
        // 两句话固化进了回归网——正是它让 D-122 带着假修复通过验收。
        let warnings = config.bash_permission_warnings();
        assert!(warnings[0].contains("全量放行"), "{warnings:?}");
        assert!(
            !warnings.iter().any(|line| line.contains("将逐次询问")),
            "实际全量放行时不得声称会询问: {warnings:?}"
        );
        assert!(
            warnings[1].contains("不生效"),
            "应说明 legacy 被覆盖: {warnings:?}"
        );
        assert_eq!(config.permissions.rules.len(), 4);
    }
    #[test]
    fn merge_layers() {
        let mut base: KanzeiConfig = toml::from_str(
            r#"
[models]
primary = "anthropic:claude-sonnet-5"
[[permissions.rules]]
action = "bash"
resource = "*"
effect = "allow"
"#,
        )
        .unwrap();
        let layer: KanzeiConfig = toml::from_str(
            r#"
[models]
primary = "kimi:kimi-k2"
[providers.kimi]
protocol = "openai"
base_url = "https://api.moonshot.cn/v1"
api_key_env = "MOONSHOT_API_KEY"
[[permissions.rules]]
action = "bash"
resource = "rm *"
effect = "deny"
"#,
        )
        .unwrap();
        merge(&mut base, layer);
        assert_eq!(base.models.primary.as_deref(), Some("kimi:kimi-k2"));
        assert_eq!(base.permissions.rules.len(), 2);
        assert!(base.providers.contains_key("kimi"));
    }

    #[test]
    fn 合并覆盖全部_models_字段_未设的键不清空() {
        // reasoning 曾被漏掉:同一个 [models] 表里 primary 生效而 reasoning 不生效,
        // 是最难查的那类不一致。
        let mut base: KanzeiConfig = toml::from_str(
            r#"
[models]
primary = "anthropic:claude-sonnet-5"
fast = "ollama:qwen3"
reasoning = "low"
"#,
        )
        .unwrap();
        let layer: KanzeiConfig = toml::from_str(
            r#"
[models]
reasoning = "high"
"#,
        )
        .unwrap();
        merge(&mut base, layer);
        assert_eq!(
            base.models.reasoning.as_deref(),
            Some("high"),
            "项目级思考强度未生效"
        );
        // 层里没设的键不得被清空——空 [models] 表不该抹掉全局配置。
        assert_eq!(
            base.models.primary.as_deref(),
            Some("anthropic:claude-sonnet-5")
        );
        assert_eq!(base.models.fast.as_deref(), Some("ollama:qwen3"));
    }

    #[test]
    fn cadence_缺节等于_conventions_1_4_默认() {
        // 验收②:旧 kanzei.toml 没有 [cadence] 节时,行为必须与 conventions §1.4
        // 当前默认逐项一致(serde default 兜底)。
        let empty: KanzeiConfig = toml::from_str("").unwrap();
        assert_eq!(empty.cadence.full_test, FullTestCadence::EntryClose);
        assert_eq!(empty.cadence.full_test_batches, None);
        assert_eq!(
            empty.cadence.targeted_test,
            TargetedTestCadence::EveryCommit
        );
        assert_eq!(empty.cadence.commit, CommitCadence::PerBatch);
        assert_eq!(empty.cadence.push, PushCadence::PerEntry);

        // 只写 [models] 的旧配置同样不引入 cadence 行为变化。
        let old: KanzeiConfig =
            toml::from_str("[models]\nprimary = \"anthropic:claude-sonnet-5\"\n").unwrap();
        assert_eq!(old.cadence.full_test, FullTestCadence::EntryClose);
        assert_eq!(old.cadence.targeted_test, TargetedTestCadence::EveryCommit);
        assert_eq!(old.cadence.push, PushCadence::PerEntry);
    }

    #[test]
    fn cadence_各档位解析与序列化() {
        // 验收①的配置侧:所有档位都能从 toml 解析并 round-trip 回原样。
        let config: KanzeiConfig = toml::from_str(
            "[cadence]\nfull_test = \"every_n_batches\"\nfull_test_batches = 3\n\
             targeted_test = \"off\"\ncommit = \"per_entry\"\npush = \"periodic\"\n",
        )
        .unwrap();
        assert_eq!(config.cadence.full_test, FullTestCadence::EveryNBatches);
        assert_eq!(config.cadence.full_test_batches, Some(3));
        assert_eq!(config.cadence.targeted_test, TargetedTestCadence::Off);
        assert_eq!(config.cadence.commit, CommitCadence::PerEntry);
        assert_eq!(config.cadence.push, PushCadence::Periodic);

        // release_only / every_commit 两种极端档位。
        let config: KanzeiConfig =
            toml::from_str("[cadence]\nfull_test = \"release_only\"\n").unwrap();
        assert_eq!(config.cadence.full_test, FullTestCadence::ReleaseOnly);
        let config: KanzeiConfig =
            toml::from_str("[cadence]\nfull_test = \"every_commit\"\n").unwrap();
        assert_eq!(config.cadence.full_test, FullTestCadence::EveryCommit);

        // 序列化 round-trip:设置页保存后重读不丢字段。
        let round: KanzeiConfig = toml::from_str(&toml::to_string(&config).unwrap()).unwrap();
        assert_eq!(round.cadence.full_test, FullTestCadence::EveryCommit);
        assert_eq!(
            round.cadence.targeted_test,
            TargetedTestCadence::EveryCommit
        );

        // 非法档位要报错,不能静默吞成默认——否则设置页拼错字用户还以为是默认生效。
        assert!(toml::from_str::<KanzeiConfig>("[cadence]\nfull_test = \"daily\"\n").is_err());
    }
}
