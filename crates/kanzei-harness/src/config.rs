//! kanzei.toml:全局(~/.kanzei/)→ 项目(.kanzei/,从 cwd 向上发现),后者覆盖前者。
//! 配置本身以组件形式进入 harness(贡献权限规则),没有第二条 config→runtime 路径。
//!
//! 按域切分(R-257 B5):limits/cadence/models/permissions/embeddings 五个纯结构域
//! 迁到子模块,本文件保留 KanzeiConfig 装配(加载/合并/默认值)+ schema 参考 + 测试。
//! 零外部 API 面变更。

mod cadence;
pub(crate) use cadence::{overlay_cadence, CADENCE_KEYS};
pub use cadence::{Cadence, CommitCadence, FullTestCadence, PushCadence, TargetedTestCadence};
mod embeddings;
pub use embeddings::EmbeddingsSection;
pub(crate) use embeddings::EMBEDDINGS_KEYS;
mod limits;
pub use limits::Limits;
pub(crate) use limits::LIMITS_KEYS;
mod models;
pub use models::{
    builtin_context_limit, builtin_provider_names, ModelRoles, ProviderConfig, ResolvedModel,
};
pub(crate) use models::{known_context_limit, MODELS_KEYS, PROVIDER_KEYS};
mod permissions;
#[cfg(test)]
use crate::permission::{Rule, BASH_ACTION};
pub use permissions::{NonInteractive, PermissionsSection, ProfileSection};
pub(crate) use permissions::{PERMISSIONS_KEYS, PERMISSION_RULE_KEYS, PROFILE_KEYS};

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct KanzeiConfig {
    /// 界面语言偏好:system/zh/en。None 保持默认中文且不写配置键。
    #[serde(default)]
    pub language: Option<String>,
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

/// 运行时上限与阈值已迁至 `config/limits.rs`;向量检索通道配置已迁至 `config/embeddings.rs`。

/// 验证与提交节奏(Cadence + 四档位枚举)已迁至 `config/cadence.rs`。

/// 模型角色与 provider 配置已迁至 `config/models.rs`。

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

        assert_eq!(config.providers["deepseek"].context_limit, Some(1_000_000));
        assert_eq!(config.providers["codex"].context_limit, Some(272_000));
        assert_eq!(config.providers["anthropic"].context_limit, Some(200_000));
        assert_eq!(config.providers["kimi"].context_limit, Some(1_000_000));
        // 认不出来的 provider 保持 None——宁可显示"未知",也不编一个预算基准。
        assert_eq!(known_context_limit("mystery", "https://example.test"), None);
    }

    /// D-246:内置名单与 fill_defaults 实际回填必须一致,否则 UI 的「内置」标记
    /// 会漏标/错标——名单漂移比没有名单更糟。
    #[test]
    fn 内置名单与fill_defaults回填一致() {
        let mut config = KanzeiConfig::default();
        config.providers.clear();
        config.fill_defaults();
        for name in builtin_provider_names() {
            assert!(
                config.providers.contains_key(*name),
                "fill_defaults 未回填内置 {name}"
            );
        }
        // 名单里不该有 fill_defaults 不保证的键(用户自定义的不能误标内置)。
        assert_eq!(config.providers.len(), builtin_provider_names().len());
    }

    #[test]
    fn deepseek旧版内置配置升级responses_自定义服务不改写() {
        let official = ProviderConfig {
            protocol: "openai".into(),
            base_url: "https://api.deepseek.com/".into(),
            api_key_env: None,
            api_key: None,
            auth: None,
            context_limit: None,
        };
        assert_eq!(
            official.effective_protocol("deepseek"),
            "deepseek-responses"
        );

        let custom = ProviderConfig {
            base_url: "https://gateway.example/v1".into(),
            ..official
        };
        assert_eq!(custom.effective_protocol("deepseek"), "openai");
    }
}

// 权限配置节(ProfileSection/PermissionsSection/NonInteractive/ResolvedModel)已迁至
// `config/permissions.rs` 与 `config/models.rs`。
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
    ///
    /// 语义不变的**发现式**入口:从 cwd 向上找项目根,再委托 [`Self::load_with_warnings_at_root`]。
    /// 显式主根(`--project-root` / `KANZEI_PROJECT_ROOT`)的调用方请直接用 at_root 版本,
    /// 别在这里绕一圈——cwd 一旦指向 worktree,发现出来的就是 worktree 自己的 `.kanzei` 副本。
    pub fn load_with_warnings(cwd: &Path) -> anyhow::Result<(KanzeiConfig, Vec<String>)> {
        let root = discover_project_root(cwd).unwrap_or_else(|| cwd.to_path_buf());
        Self::load_with_warnings_at_root(&root)
    }

    /// 显式主根加载:直接叠加全局 `kanzei_home()/kanzei.toml` 与
    /// `project_root/.kanzei/kanzei.toml`,**不做任何根发现**。
    ///
    /// 与 [`Self::load`] 的区别只有一条:根从哪来。这里的根是调用方说了算的,
    /// 因此在 worktree 里也能读到主根那份配置(D-267:worktree 里 `.kanzei` 是
    /// 被 git checkout 出来的**分支副本**,发现式取根会读到过期的那一份)。
    pub fn load_at_root(project_root: &Path) -> anyhow::Result<KanzeiConfig> {
        let (config, warnings) = Self::load_with_warnings_at_root(project_root)?;
        for warning in &warnings {
            tracing::warn!("{warning}");
        }
        Ok(config)
    }

    /// 同 [`Self::load_at_root`],但把未知字段告警交给调用方展示。
    pub fn load_with_warnings_at_root(
        project_root: &Path,
    ) -> anyhow::Result<(KanzeiConfig, Vec<String>)> {
        let mut config = KanzeiConfig::default();
        let mut warnings = Vec::new();
        if let Some(home) = crate::home::kanzei_home() {
            merge_file(&mut config, &home.join("kanzei.toml"), &mut warnings)?;
        }
        merge_file(
            &mut config,
            &project_root.join(".kanzei").join("kanzei.toml"),
            &mut warnings,
        )?;
        config.fill_defaults();
        // 三处接线之三:非交互策略键认不出来时的 fail-closed 告警。要在**层叠之后**打,
        // 因为最终生效的是合并结果——全局写对了、项目层写错了,该报的是项目层那个值。
        warnings.extend(config.non_interactive_policy_warning());
        Ok((config, warnings))
    }

    // 权限相关方法(legacy bash 规则/非交互策略/启动告警)已迁至 `config/permissions.rs`。
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
        // DeepSeek 原生 Responses API。
        self.providers
            .entry("deepseek".into())
            .or_insert(ProviderConfig {
                protocol: "deepseek-responses".into(),
                base_url: "https://api.deepseek.com".into(),
                api_key_env: Some("DEEPSEEK_API_KEY".into()),
                api_key: None,
                auth: None,
                context_limit: Some(1_000_000),
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
            // R-236 B3:compact 未配置回落 primary(纪要默认跟随主模型)。
            "compact" => self
                .models
                .compact
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .or(self.models.primary.as_deref())
                .unwrap_or_default(),
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

// 内置 provider 名单与出厂 context_limit 已迁至 `config/models.rs`。
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
        // D-245:cadence 层叠。字段非 Option,必须用 raw 表显式键驱动 overlay,
        // 否则「项目层只写一个键」会把其余字段打回默认(见 overlay_cadence)。
        let cadence_written: std::collections::HashSet<&str> = raw
            .get("cadence")
            .and_then(|v| v.as_table())
            .map(|table| table.keys().map(String::as_str).collect())
            .unwrap_or_default();
        if !cadence_written.is_empty() {
            overlay_cadence(&mut config.cadence, &layer.cadence, &cadence_written);
        }
    }
    merge(config, layer);
    Ok(())
}

/// R-205:权限规则持久化(append_allow_rule/generalize_resource/rule_digest/通配判定)
/// 已拆至 `crate::permission_persist`,re-export 保持调用点零变更。
pub use crate::permission_persist::{append_allow_rule, generalize_resource};

/// kanzei.toml 各节已知键名单(R-220 单源)。
///
/// `unknown_keys` 的已知清单、用户面配置参考 `config_reference` 都从这里取——
/// 增删键只改一处,`config_reference_covers_all_known_keys` 测试守护参考不丢键,
/// `unknown_keys_schema_matches_struct` 守护名单不与结构体漂移。
pub(crate) const TOP_LEVEL_KEYS: &[&str] = &[
    "language",
    "models",
    "providers",
    "proxy",
    "profile",
    "permissions",
    "limits",
    "cadence",
    "embeddings",
];
// 各节已知键名单随域下沉:MODELS_KEYS/EMBEDDINGS_KEYS/LIMITS_KEYS/PROVIDER_KEYS/
// PROFILE_KEYS/CADENCE_KEYS/PERMISSIONS_KEYS/PERMISSION_RULE_KEYS 见对应域文件
// (config/{models,embeddings,limits,cadence,permissions}.rs)。

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
    check(value, "", TOP_LEVEL_KEYS, &mut out);
    if let Some(models) = value.get("models") {
        check(models, "models", MODELS_KEYS, &mut out);
    }
    if let Some(embeddings) = value.get("embeddings") {
        check(embeddings, "embeddings", EMBEDDINGS_KEYS, &mut out);
    }
    if let Some(limits) = value.get("limits") {
        check(limits, "limits", LIMITS_KEYS, &mut out);
    }
    if let Some(providers) = value.get("providers").and_then(|p| p.as_table()) {
        for (name, provider) in providers {
            check(
                provider,
                &format!("providers.{name}"),
                PROVIDER_KEYS,
                &mut out,
            );
        }
    }
    if let Some(profile) = value.get("profile") {
        check(profile, "profile", PROFILE_KEYS, &mut out);
    }
    if let Some(cadence) = value.get("cadence") {
        check(cadence, "cadence", CADENCE_KEYS, &mut out);
    }
    if let Some(permissions) = value.get("permissions") {
        // 三处接线之一:新键要进这份已知键清单,否则用户一写就收到"未知配置项"假告警。
        check(permissions, "permissions", PERMISSIONS_KEYS, &mut out);
        if let Some(rules) = permissions.get("rules").and_then(|r| r.as_array()) {
            for (index, rule) in rules.iter().enumerate() {
                check(
                    rule,
                    &format!("permissions.rules[{index}]"),
                    PERMISSION_RULE_KEYS,
                    &mut out,
                );
            }
        }
    }
    out
}

/// R-220:用户面配置参考——覆盖全部已知键,一句话说明 + 默认值/取值范围。
///
/// 由上方各节已知键常量驱动生成:增删键只改常量,参考自动跟随;
/// `config_reference_covers_all_known_keys` 测试守护参考不丢键、不凭空多键。
/// 输出是纯注释 TOML(每个键一行 `# 键 = 默认/取值 说明`),可直接作为
/// `kz config schema` 命令的 stdout。
pub fn config_reference() -> String {
    let mut out = String::from(
        "# kanzei.toml 配置参考(R-220)。留空 = 用内置默认;设置页改的也是这个文件。\n\
         # 取值用 | 分隔合法项;默认值是内置默认。\n\
         # language = system | zh | en  界面语言(默认 zh)\n\
         #\n",
    );
    let models_desc = |key: &str| -> &str {
        match key {
            "primary" => " = <角色名或 provider:model>  主对话模型(默认 unset,回退内置)",
            "fast" => " = <角色名或 provider:model>  快速子代理/机械检索(默认 unset)",
            "scout" => " = <角色名或 provider:model>  勘察/复核只读代理(默认跟随 fast)",
            "compact" => " = <角色名或 provider:model>  上下文压缩纪要(默认跟随 primary)",
            "reasoning" => " = off | low | medium | high  思考强度(默认 off)",
            "codex_fast_mode" => " = true | false  同模型走高消耗 priority 档(默认未设)",
            _ => "",
        }
    };
    let provider_desc = |key: &str| -> &str {
        match key {
            "protocol" => " = anthropic | openai | openai-responses | deepseek-responses",
            "base_url" => " = https://...  端点地址(必填)",
            "api_key_env" => " = <环境变量名>  从环境变量取密钥(推荐)",
            "api_key" => " = <明文>  直填密钥,明文存盘自担风险",
            "auth" => " = codex | claude  复用 CLI 登录态(可选)",
            "context_limit" => " = <u64>  上下文窗口 token 数(默认由 builtin_context_limit 决定)",
            _ => "",
        }
    };
    let limits_desc = |key: &str| -> &str {
        match key {
            "max_tokens" => " = <u32>  单轮输出上限(默认 4096)",
            "subagent_max_tokens" => " = <u32>  子代理输出上限(默认 4096)",
            "subagent_timeout_secs" => " = <u64>  子代理超时秒数(默认 900)",
            "barrier_timeout_secs" => " = <u64>  阶段屏障超时秒数(默认 3600)",
            "context_budget_ratio" => " = <f64 0.0~1.0>  上下文预算比例(默认 0.7)",
            "recent_verbatim_ratio" => " = <f64 0.0~1.0>  最近内容原样保留比例(默认 0.35)",
            "max_tasks_per_turn" => " = <usize>  单轮并行子代理上限(默认 8)",
            "max_parallel_tools" => " = <usize>  单轮并行工具上限(默认 8)",
            "transport_retries" => " = <u32>  传输重试次数(默认 2)",
            "rate_limit_retries" => " = <u32>  限流重试次数(默认 2)",
            "stream_restarts" => " = <u32>  流重启次数(默认 2)",
            "compact_buffer_tokens" => " = <u64>  压缩保留缓冲 token(默认 8000)",
            "prune_protect_tokens" => " = <u64>  裁剪保护 token(默认 2000)",
            "prune_min_gain_tokens" => " = <u64>  裁剪最小收益 token(默认 512)",
            _ => "",
        }
    };
    let cadence_desc = |key: &str| -> &str {
        match key {
            "full_test" => {
                " = entry_close | every_commit | every_n_batches | release_only(默认 entry_close)"
            }
            "full_test_batches" => " = <u32>  full_test=every_n_batches 时的批次间隔(默认 null)",
            "targeted_test" => " = every_commit | off(默认 every_commit)",
            "commit" => " = per_batch | per_entry(默认 per_batch)",
            "push" => " = per_entry | per_commit | periodic(默认 per_entry)",
            "verify_every_n" => " = <u32>  自主推进每关 N 条插入只读核查;0=关闭(默认 3)",
            _ => "",
        }
    };
    let profile_desc = |key: &str| -> &str {
        match key {
            "default" => " = dev | research | readonly(默认 dev)",
            _ => "",
        }
    };
    let permissions_desc = |key: &str| -> &str {
        match key {
            "rules" => " = [{ action, resource, effect }, ...]  有序规则,last-match-wins",
            "non_interactive" => " = deny | rules_only | allow_listed(默认 deny)",
            _ => "",
        }
    };
    let rule_desc = |key: &str| -> &str {
        match key {
            "action" => " = bash | write | edit | ...",
            "resource" => " = <资源描述>",
            "effect" => " = allow | deny",
            _ => "",
        }
    };
    let embeddings_desc = |key: &str| -> &str {
        match key {
            "provider" => " = <provider 名>  嵌入通道(默认未设=不启用)",
            "model" => " = <模型名>  嵌入模型(默认未设)",
            _ => "",
        }
    };
    emit_section(&mut out, "models", MODELS_KEYS, &models_desc);
    emit_section(&mut out, "providers.<名字>", PROVIDER_KEYS, &provider_desc);
    emit_section(&mut out, "limits", LIMITS_KEYS, &limits_desc);
    out.push_str("# [proxy]\n#   proxy = env | off | http://host:port  (默认 env,读环境变量)\n\n");
    emit_section(&mut out, "cadence", CADENCE_KEYS, &cadence_desc);
    emit_section(&mut out, "profile", PROFILE_KEYS, &profile_desc);
    emit_section(&mut out, "permissions", PERMISSIONS_KEYS, &permissions_desc);
    out.push_str("#   [[permissions.rules]]\n");
    for key in PERMISSION_RULE_KEYS {
        out.push_str(&format!("#     {key}{}\n", rule_desc(key)));
    }
    out.push_str("#\n");
    emit_section(&mut out, "embeddings", EMBEDDINGS_KEYS, &embeddings_desc);
    out
}

/// 把一节已知键 + 描述写成参考注释行。
fn emit_section(
    out: &mut String,
    name: &str,
    keys: &[&str],
    describe: &dyn Fn(&str) -> &'static str,
) {
    out.push_str(&format!("# [{name}]\n"));
    for key in keys {
        out.push_str(&format!("#   {key}{}\n", describe(key)));
    }
    out.push('\n');
}

/// 标量覆盖、map 合并、规则追加(后层排后 → last-match-wins 自然让后层优先)。
fn merge(base: &mut KanzeiConfig, layer: KanzeiConfig) {
    if layer.language.is_some() {
        base.language = layer.language;
    }
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
    if layer.models.scout.is_some() {
        base.models.scout = layer.models.scout;
    }
    if layer.models.compact.is_some() {
        base.models.compact = layer.models.compact;
    }
    base.providers.extend(layer.providers);
    if layer.proxy.is_some() {
        base.proxy = layer.proxy;
    }
    if layer.profile.default.is_some() {
        base.profile.default = layer.profile.default;
    }
    base.permissions.rules.extend(layer.permissions.rules);
    // 三处接线之二:标量键要按 [limits] 的同一套规矩做 overlay —— 项目层写了才覆盖,
    // 没写就保持全局层的值。只加字段不接这一行,项目层设了会静默不生效
    // (`Limits::barrier_timeout_secs` 就是这么漏的,D-300 已补,穷举守护测试在防复发)。
    if layer.permissions.non_interactive.is_some() {
        base.permissions.non_interactive = layer.permissions.non_interactive;
    }
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
        barrier_timeout_secs,
        context_budget_ratio,
        recent_verbatim_ratio,
        max_tasks_per_turn,
        max_parallel_tools,
        transport_retries,
        rate_limit_retries,
        stream_restarts,
        compact_buffer_tokens,
        prune_protect_tokens,
        prune_min_gain_tokens,
    );
    // [embeddings] 逐字段覆盖(与 [limits] 同规:项目层只覆盖写了的那几个键)。
    if layer.embeddings.provider.is_some() {
        base.embeddings.provider = layer.embeddings.provider;
    }
    if layer.embeddings.model.is_some() {
        base.embeddings.model = layer.embeddings.model;
    }
}

/// R-205:权限规则持久化与根发现/HOME 守卫两域已从本文件拆出
/// (append_allow_rule/generalize_resource → `crate::permission_persist`;
/// discover_project_root/resolve_project_root/is_home_root → `crate::project_root`),
/// 以下 re-export 保持 `config::xxx` 调用点零变更。
pub use crate::project_root::{
    discover_project_config, discover_project_root, is_home_root, resolve_project_root,
};

/// R-178 P2:五层模型解析链的前三层(引用层)合并——本轮直选 → 线持久选择 → agent 默认。
///
/// 五层链(design §3.4):
/// ① 本轮直选(`run_prompt` 的 `model` 参数 / CLI `KANZEI_MODEL`);
/// ② 线/进程持久选择(`ProcessHandle.model`,state.db `processes` 表,重启后恢复);
/// ③ agent 定义的默认引用(通常 `"primary"` / `"fast"` 角色);
/// ④ 项目 `[models]` / 全局 `[models]` 层叠——由 `KanzeiConfig::load_with_warnings`
///    (全局 merge → 项目覆盖)在 `config.models` 里完成;
/// ⑤ 内置默认——由 `fill_defaults()` 兜底。
///
/// 本函数只负责 ①②③ 的**引用合并**:返回最终要交给 `resolve_model` 的模型引用串。
/// ④⑤ 是 `resolve_model` 内部的事(角色 → provider:model → [providers]),
/// 本函数不掺和。CLI 与桌面共用这一份,保证「同一真源」(验收②)。
///
/// 空串 / 纯空白视为「未设」,逐层回落。
pub fn resolve_model_chain(
    run_override: Option<&str>,
    process_model: Option<&str>,
    agent_model: &str,
) -> String {
    run_override
        .filter(|value| !value.trim().is_empty())
        .or_else(|| process_model.filter(|value| !value.trim().is_empty()))
        .map(str::to_string)
        .unwrap_or_else(|| agent_model.to_string())
}

/// R-205:五层链 ①②③ 引用合并见上;④⑤ 由 load_with_warnings + fill_defaults 承担。
/// 根发现/显式主根/HOME 守卫(含目录身份判定)已拆至 `crate::project_root`
/// (re-export 见文件头,D-270 修复落点在该文件)。
#[cfg(test)]
mod tests {
    use super::*;
    // R-205:project_root 域测试仍驻留本文件,经 glob 导入 crate::project_root 的
    // pub(crate) 判定函数(实现已单源在 project_root.rs)。
    use crate::project_root::*;
    use std::path::PathBuf;

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

    /// R-178 P2 五层链 ①②③:本轮直选 → 线持久选择 → agent 默认,逐层缺省回落。
    /// (④项目/全局 [models] 与 ⑤内置默认由 load_with_warnings + fill_defaults 承担,
    /// 见 merge_layers 与 defaults_and_model_resolution。)
    #[test]
    fn 五层链引用层逐层缺省回落() {
        // ① 本轮直选存在 → 用它(即使线持久与 agent 默认都设了)。
        assert_eq!(
            resolve_model_chain(Some("claude:claude-sonnet-4-6"), Some("a:b"), "primary"),
            "claude:claude-sonnet-4-6"
        );
        // ① 缺省 → ② 线持久选择。
        assert_eq!(resolve_model_chain(None, Some("a:b"), "primary"), "a:b");
        // ①② 都缺省 → ③ agent 默认。
        assert_eq!(resolve_model_chain(None, None, "primary"), "primary");
        // 空串视为未设,照样回落。
        assert_eq!(
            resolve_model_chain(Some("   "), Some("a:b"), "primary"),
            "a:b"
        );
        assert_eq!(
            resolve_model_chain(None, Some("  "), "fast"),
            "fast",
            "空白线持久值不得覆盖 agent 默认"
        );
    }

    /// R-173:勘察/复核路由可配,缺省沿用 fast(旧配置行为逐字节不变)。
    #[test]
    fn 勘察路由可配且缺省沿用fast() {
        // 旧配置没有这个键 → None,调用方回退 fast。
        let old: KanzeiConfig = toml::from_str("[models]\nprimary = \"a:b\"\n").unwrap();
        assert_eq!(old.models.scout, None, "缺省必须是 None,不能替用户拍板");

        // 显式配置能解析成真实模型(与 primary/fast 同一套解析,没有第二套)。
        let mut c: KanzeiConfig =
            toml::from_str("[models]\nscout = \"claude:claude-sonnet-4-6\"\n").unwrap();
        c.fill_defaults();
        let resolved = c
            .resolve_model(c.models.scout.as_deref().unwrap())
            .expect("scout 取值必须走既有 resolve_model");
        assert_eq!(resolved.model, "claude-sonnet-4-6");
        // 角色名同样能用。
        let role: KanzeiConfig = toml::from_str("[models]\nscout = \"primary\"\n").unwrap();
        assert_eq!(role.models.scout.as_deref(), Some("primary"));

        // 层叠:项目层写了才覆盖,没写不打回默认(reasoning 那次漏合并的教训)。
        let mut base: KanzeiConfig =
            toml::from_str("[models]\nprimary = \"a:b\"\nscout = \"fast\"\n").unwrap();
        let layer: KanzeiConfig = toml::from_str("[models]\nprimary = \"c:d\"\n").unwrap();
        merge(&mut base, layer);
        assert_eq!(
            base.models.scout.as_deref(),
            Some("fast"),
            "项目层没写 scout 时必须保住全局层的值"
        );
        let layer2: KanzeiConfig = toml::from_str("[models]\nscout = \"primary\"\n").unwrap();
        merge(&mut base, layer2);
        assert_eq!(base.models.scout.as_deref(), Some("primary"));

        // 未知键体检不得把 scout 报成拼错(设置页透传不丢字段)。
        let value: toml::Value = toml::from_str("[models]\nscout = \"primary\"\n").unwrap();
        let warnings = unknown_keys(&value);
        assert!(
            !warnings.iter().any(|w| w.contains("scout")),
            "scout 是已知键,不该被报成未知: {warnings:?}"
        );
    }

    /// R-236 B3:compact 角色——缺省回落 **primary**(不是 fast:弱模型纪要有
    /// -8pp 实测消融),显式配置走独立解析;层叠与未知键体检同 scout 一套规矩。
    #[test]
    fn compact_角色_缺省回落primary_显式配置与层叠生效() {
        // 缺省:resolve_model("compact") 必须解析到 primary 指向的模型。
        let mut c: KanzeiConfig =
            toml::from_str("[models]\nprimary = \"deepseek:dsv4\"\n").unwrap();
        c.fill_defaults();
        let resolved = c.resolve_model("compact").expect("缺省必须回落 primary");
        assert_eq!(resolved.model, "dsv4");
        assert_eq!(resolved.provider_name, "deepseek");
        // 空串视同未设,同样回落。
        let mut blank: KanzeiConfig =
            toml::from_str("[models]\nprimary = \"deepseek:dsv4\"\ncompact = \"  \"\n").unwrap();
        blank.fill_defaults();
        assert_eq!(blank.resolve_model("compact").unwrap().model, "dsv4");
        // 显式配置:走自己的指向,不再跟随 primary。
        let mut explicit: KanzeiConfig = toml::from_str(
            "[models]\nprimary = \"deepseek:dsv4\"\ncompact = \"claude:claude-sonnet-4-6\"\n",
        )
        .unwrap();
        explicit.fill_defaults();
        assert_eq!(
            explicit.resolve_model("compact").unwrap().model,
            "claude-sonnet-4-6"
        );
        // 层叠:项目层没写 compact 不得打回默认;写了才覆盖。
        let mut base: KanzeiConfig =
            toml::from_str("[models]\nprimary = \"a:b\"\ncompact = \"fast\"\n").unwrap();
        let layer: KanzeiConfig = toml::from_str("[models]\nprimary = \"c:d\"\n").unwrap();
        merge(&mut base, layer);
        assert_eq!(base.models.compact.as_deref(), Some("fast"));
        let layer2: KanzeiConfig = toml::from_str("[models]\ncompact = \"primary\"\n").unwrap();
        merge(&mut base, layer2);
        assert_eq!(base.models.compact.as_deref(), Some("primary"));
        // 未知键体检:compact 是已知键。
        let value: toml::Value = toml::from_str("[models]\ncompact = \"primary\"\n").unwrap();
        let warnings = unknown_keys(&value);
        assert!(
            !warnings.iter().any(|w| w.contains("compact")),
            "compact 是已知键,不该被报成未知: {warnings:?}"
        );
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
        assert_eq!(empty.limits.max_tasks_per_turn(), 16);

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

    /// F8 ①(D-139 的新形态):**探针 Allow ≠ yolo**。探针是一条具体命令,用户完全可能
    /// 只授权过它自己;老判据会把"你授权过 git status"说成"bash 已经全放开",又一句假话。
    #[test]
    fn 探针被单条规则命中时不得冒充全量放行() {
        // 规则内容与探针逐字节相同:探针必然 Allow,但配置里没有任何 `bash/*`。
        let probe = serde_json::json!({ "command": "git status", "workdir": "." }).to_string();
        let text = format!(
            "[[permissions.rules]]\naction = \"bash\"\nresource = '{probe}'\neffect = \"allow\"\n"
        );
        let config: KanzeiConfig = toml::from_str(&text).unwrap();

        // 先钉住前提:这条配置下探针的**实际评估结果**确实是 Allow,
        // 否则这条用例根本没走到被修的那个分支。
        let mut ruleset = crate::permission::Ruleset::default();
        for rule in &config.permissions.rules {
            ruleset.push(rule.clone());
        }
        assert_eq!(
            ruleset.evaluate(BASH_ACTION, &probe),
            crate::permission::Effect::Allow
        );
        assert_eq!(config.explicit_bash_wildcard_allows().len(), 0);

        let w = config.bash_permission_warnings();
        assert!(
            !w.iter().any(|line| line.contains("全量放行")),
            "没有 bash/* 规则就不许说全量放行: {w:?}"
        );
        assert!(
            w.iter().any(|line| line.contains("其余命令仍会逐次询问")),
            "必须如实说清范围: {w:?}"
        );
    }

    /// D-269 收敛路径(验收⑥):停止对 bash 资源做路径规范化之后,**已落盘的结构化规则
    /// 里有一类是可解析的**——命令文本被 Windows 整串小写改写过,JSON 还合法,于是
    /// `legacy_bash_rules` 一条都认不出来:用户零告警、零指引,只看到命令又开始逐次询问。
    ///
    /// 夹具用的是本机真实配置里的形态(9 条非结构化 / 7 条被折成不可解析 / 5 条可解析,
    /// 其中 3 条命令原文有大写因而已经失效)。
    #[test]
    fn 被小写折坏的结构化规则也要进告警() {
        let text = r#"
# ① 非结构化(裸命令):legacy_bash_rules 认得
[[permissions.rules]]
action = "bash"
resource = "Get-ChildItem *"
effect = "allow"

# ② 结构化但被 `\` → `/` 折成不可解析:legacy_bash_rules 也认得
[[permissions.rules]]
action = "bash"
resource = '{"command":"git commit -m /"整理/"","workdir":"c:/proj"}'
effect = "allow"

# ③ 结构化、可解析、但命令原文有大写被整串小写:老判据完全认不出来
[[permissions.rules]]
action = "bash"
resource = '{"command":"(get-content /x/main.rs).count","workdir":"c:/proj"}'
effect = "allow"

# ④ 结构化、可解析、天生全小写:其实还活着,判据只能宽报(理由见函数注释)
[[permissions.rules]]
action = "bash"
resource = '{"command":"git status --short","workdir":"c:/proj"}'
effect = "allow"

# ⑤ deny 是仍然生效的护栏,不算失效(D-139)
[[permissions.rules]]
action = "bash"
resource = '{"command":"rm -rf /","workdir":"c:/proj"}'
effect = "deny"
"#;
        let config: KanzeiConfig = toml::from_str(text).unwrap();

        // ③④ 正是老判据漏掉的那一类:它们是合法结构化资源,legacy 那边一条不收。
        let legacy = config.legacy_bash_rules_needing_downgrade();
        assert_eq!(legacy.len(), 2, "①② 归 legacy: {legacy:?}");

        let stale = config.structured_bash_rules_possibly_stale();
        assert_eq!(stale.len(), 2, "③④ 归新判据,deny 的 ⑤ 不算: {stale:?}");
        assert!(stale.iter().any(|r| r.resource.contains("get-content")));

        let w = config.bash_permission_warnings();
        let text = w.join("\n");
        // 两类各说各的,数目都要出现。
        assert!(text.contains("2 条旧 bash 权限规则"), "{w:?}");
        assert!(text.contains("2 条结构化 bash 规则"), "{w:?}");
        // 可执行的动作:说清失效 + 下次遇到重新授权一次即可。
        assert!(text.contains("总是允许"), "必须给出可照做的动作: {w:?}");
        // 点名到具体命令,而不是只报一个数字。
        assert!(text.contains("Get-ChildItem *"), "{w:?}");
        assert!(text.contains("get-content"), "{w:?}");
        // 不许再出现那句没有落点的老文案。
        assert!(!text.contains("请重新选择精确作用域"), "{w:?}");
    }

    /// F8 ⑥ 接线之三:非交互策略键缺席 = `deny` = **今天的行为**,旧配置逐字节不变。
    #[test]
    fn 非交互策略缺键等于deny且旧配置行为不变() {
        let empty: KanzeiConfig = toml::from_str("").unwrap();
        assert_eq!(empty.non_interactive_policy(), NonInteractive::Deny);
        assert!(empty.non_interactive_policy_warning().is_none());

        // 只有 rules 的老配置同样不变,也不产额外告警。
        let old: KanzeiConfig = toml::from_str(
            "[[permissions.rules]]\naction = \"bash\"\nresource = \"*\"\neffect = \"allow\"\n",
        )
        .unwrap();
        assert_eq!(old.non_interactive_policy(), NonInteractive::Deny);
        assert!(old.non_interactive_policy_warning().is_none());

        for (写法, 期望) in [
            ("deny", NonInteractive::Deny),
            ("rules_only", NonInteractive::RulesOnly),
            ("allow_listed", NonInteractive::AllowListed),
            ("  Rules_Only  ", NonInteractive::RulesOnly),
        ] {
            let c: KanzeiConfig =
                toml::from_str(&format!("[permissions]\nnon_interactive = \"{写法}\"\n")).unwrap();
            assert_eq!(c.non_interactive_policy(), 期望, "写法 {写法:?}");
            assert!(
                c.non_interactive_policy_warning().is_none(),
                "写法 {写法:?}"
            );
        }
    }

    /// F8 ⑥ 接线之三(续):认不出来的取值 **fail-closed 回落 deny 并且必须出声**。
    /// 悄悄回落最坏——用户以为开了 rules_only,实际每次停机,还归不到因。
    #[test]
    fn 非交互策略非法取值fail_closed回落deny并告警() {
        for 写法 in ["rulesonly", "yolo", "true", "1"] {
            let c: KanzeiConfig =
                toml::from_str(&format!("[permissions]\nnon_interactive = \"{写法}\"\n")).unwrap();
            assert_eq!(
                c.non_interactive_policy(),
                NonInteractive::Deny,
                "非法取值 {写法:?} 必须回落 deny"
            );
            let warning = c
                .non_interactive_policy_warning()
                .unwrap_or_else(|| panic!("非法取值 {写法:?} 必须产告警"));
            assert!(warning.contains(写法), "告警要点名原值: {warning}");
            assert!(warning.contains("fail-closed"), "{warning}");
            assert!(warning.contains("rules_only"), "要列出可用取值: {warning}");
        }
        // 空串按"没写"处理:不回落告警,行为等于缺键。
        let 空: KanzeiConfig = toml::from_str("[permissions]\nnon_interactive = \"  \"\n").unwrap();
        assert_eq!(空.non_interactive_policy(), NonInteractive::Deny);
        assert!(空.non_interactive_policy_warning().is_none());
    }

    /// F8 ⑥ 接线之二:`merge` 的 overlay。只加字段不接这一行,项目层设了会静默不生效
    /// (`Limits::barrier_timeout_secs` 的前车之鉴)。
    #[test]
    fn 非交互策略项目层覆盖全局层() {
        let mut base: KanzeiConfig =
            toml::from_str("[permissions]\nnon_interactive = \"deny\"\n").unwrap();
        let layer: KanzeiConfig =
            toml::from_str("[permissions]\nnon_interactive = \"rules_only\"\n").unwrap();
        merge(&mut base, layer);
        assert_eq!(base.non_interactive_policy(), NonInteractive::RulesOnly);

        // 反向:项目层没写这个键,不能把全局层的值打回默认。
        let mut base: KanzeiConfig =
            toml::from_str("[permissions]\nnon_interactive = \"allow_listed\"\n").unwrap();
        let layer: KanzeiConfig = toml::from_str(
            "[[permissions.rules]]\naction = \"bash\"\nresource = \"*\"\neffect = \"allow\"\n",
        )
        .unwrap();
        merge(&mut base, layer);
        assert_eq!(base.non_interactive_policy(), NonInteractive::AllowListed);
        assert_eq!(base.permissions.rules.len(), 1, "rules 仍然是追加语义");
    }

    /// 序列化顺序陷阱:TOML 里标量键必须排在**数组表之前**,否则 `non_interactive` 会被
    /// 当成 `[[permissions.rules]]` 最后一项的字段,写出去的文件读回来就变了个意思。
    /// `unknown_keys_schema_matches_struct` 只覆盖了缺省(None)那种情况,盖不住这条。
    #[test]
    fn 非交互策略键与规则数组同时存在时能原样round_trip() {
        let mut config = KanzeiConfig::default();
        config.permissions.non_interactive = Some("rules_only".into());
        config.permissions.rules.push(Rule {
            action: "bash".into(),
            resource: "*".into(),
            effect: crate::permission::Effect::Allow,
        });
        let text = toml::to_string_pretty(&config).unwrap();
        let back: KanzeiConfig = toml::from_str(&text)
            .unwrap_or_else(|e| panic!("序列化产物读不回来(标量排在数组表之后?): {e}\n{text}"));
        assert_eq!(back.non_interactive_policy(), NonInteractive::RulesOnly);
        assert_eq!(back.permissions.rules.len(), 1);
        let raw: toml::Value = toml::from_str(&text).unwrap();
        assert_eq!(unknown_keys(&raw), Vec::<String>::new());
    }

    /// F8 ⑥ 接线之一:新键要进 `unknown_keys` 的已知清单,否则一写就收到"未知配置项"假告警。
    #[test]
    fn 非交互策略键不产生未知配置项假告警() {
        let raw: toml::Value =
            toml::from_str("[permissions]\nnon_interactive = \"rules_only\"\n").unwrap();
        assert_eq!(unknown_keys(&raw), Vec::<String>::new());
        // 同一节里拼错的键仍然要报出来,清单不是把整节放行。
        let raw: toml::Value =
            toml::from_str("[permissions]\nnon_interactiv = \"rules_only\"\n").unwrap();
        assert_eq!(unknown_keys(&raw), vec!["permissions.non_interactiv"]);
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

    /// R-220 验收②:配置参考与 known_keys 名单必须同源不漂移。
    /// 每个已知键都在参考里出现一次,参考里也不能出现名单外的键。
    #[test]
    fn config_reference_covers_all_known_keys() {
        let reference = config_reference();
        // R-220 验收③:D-300 修复后的键必须能在用户面参考里看到。
        assert!(
            reference.contains("barrier_timeout_secs"),
            "config_reference 缺 barrier_timeout_secs(D-300 修复键必须可见):\n{reference}"
        );
        let mut all_keys: Vec<&str> = Vec::new();
        all_keys.extend(TOP_LEVEL_KEYS.iter().copied());
        all_keys.extend(MODELS_KEYS.iter().copied());
        all_keys.extend(EMBEDDINGS_KEYS.iter().copied());
        all_keys.extend(LIMITS_KEYS.iter().copied());
        all_keys.extend(PROVIDER_KEYS.iter().copied());
        all_keys.extend(PROFILE_KEYS.iter().copied());
        all_keys.extend(CADENCE_KEYS.iter().copied());
        all_keys.extend(PERMISSIONS_KEYS.iter().copied());
        all_keys.extend(PERMISSION_RULE_KEYS.iter().copied());
        for key in &all_keys {
            let needle = if TOP_LEVEL_KEYS.contains(key) {
                // 顶层键:language 是标量(# language = ...),providers 是动态节
                // (# [providers.<名字>]),其余是静态节(# [models] 等)。
                if *key == "language" {
                    format!("# {key} =")
                } else if *key == "providers" {
                    "# [providers.<名字>]".to_string()
                } else {
                    format!("# [{key}]")
                }
            } else if PERMISSION_RULE_KEYS.contains(key) {
                format!("#     {key}")
            } else {
                format!("#   {key}")
            };
            assert!(
                reference.contains(&needle),
                "config_reference 缺少已知键「{key}」(needle `{needle}`)——增删键必须同步更新参考:\n{reference}"
            );
        }
        // 反向:参考里以 `#   <键>` 开头的键必须在名单里(防参考凭空多键)。
        for line in reference.lines() {
            let Some(tail) = line.strip_prefix("#   ") else {
                continue;
            };
            let key = tail.split_whitespace().next().unwrap_or("");
            // `#   [[permissions.rules]]` 是数组表头不是键;键行形如 `#   key = ...`。
            if key.starts_with('[') || key.starts_with("]]") {
                continue;
            }
            assert!(
                all_keys.contains(&key),
                "config_reference 出现名单外的键「{key}」——名单增删后参考也要同步:\n{line}"
            );
        }
        // 反向:参考里以 `# [<节>]` 开头的节名必须是顶层已知键。
        for line in reference.lines() {
            let Some(name) = line
                .strip_prefix("# [")
                .and_then(|rest| rest.strip_suffix(']'))
            else {
                continue;
            };
            // 动态节名(providers.<名字>)与数组表(permissions.rules)不是顶层已知键,跳过。
            if !name.chars().all(|c| c.is_ascii_lowercase()) {
                continue;
            }
            assert!(
                TOP_LEVEL_KEYS.contains(&name),
                "config_reference 出现名单外的顶层节「{name}」——顶层键增删后参考也要同步:\n{line}"
            );
        }
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

    /// D-270 缺口①:发现式取根对**别名形态**的 HOME 也要拦得住——`dir_key` 词法
    /// 认不出尾随点/UNC 这类别名(那是 [`is_same_dir`] 的身份层职责),原来的实现
    /// 会把别名 HOME 当项目根返回。修复后 `.kanzei` 标记层若是 HOME 别名,同样
    /// 跳过继续向上。
    #[test]
    #[cfg(windows)]
    fn 发现式取根对别名形态的home也拦得住() {
        let root = project_root_fixture("discover-alias-home");
        let home = root.join("home");
        let plain = home.join("scratch");
        std::fs::create_dir_all(&plain).unwrap();
        // 尾随点在 Windows 磁盘上就是同一个目录:夹具有效性先验一遍。
        let alias = PathBuf::from(format!("{}.", home.display()));
        assert!(is_same_dir(&alias, &home), "夹具失效:别名必须是同一目录");
        assert_ne!(dir_key(&alias), dir_key(&home), "夹具失效:词法上本该不同");
        assert_ne!(
            discover_project_root_with_home(&plain, Some(&alias)),
            Some(home.clone()),
            "别名形态的 HOME 被当成了项目根(缺口①)"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    /// D-270 缺口②:卷元数据读失败时**保守判同**(可能相同),不再 fail-open 放行
    /// UNC 别名。原实现 fingerprint 拿不到就 return false = 判成不同 = 放行,
    /// 与它自己的注释「只会偏保守」相悖。
    #[test]
    fn 卷元数据读失败时保守判同而不是放行() {
        let 甲 = std::env::temp_dir().join(format!(
            "kanzei-d270-meta-a-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let 乙 = std::env::temp_dir().join(format!(
            "kanzei-d270-meta-b-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        assert!(!甲.exists() && !乙.exists(), "夹具失效:两个路径必须不存在");
        assert!(
            same_dir_by_volume_metadata(&甲, &乙),
            "读失败必须保守判同(拿不到身份 = 可能相同,由上层保守处置)"
        );
        // 正常目录语义不变:同目录 true、异目录 false。
        let root = project_root_fixture("meta-conservative");
        let a = root.join("home");
        let b = root.join("home").join("projects");
        assert!(same_dir_by_volume_metadata(&a, &a));
        assert!(!same_dir_by_volume_metadata(&a, &b));
        std::fs::remove_dir_all(root).unwrap();
    }

    /// D-270 缺口③:`KANZEI_HOME` 参与比较。全局根(KANZEI_HOME 或默认 `~/.kanzei`)
    /// 与项目根本身或其 `.kanzei` 同目录时,项目产物会写进全局配置根,必须被拦。
    /// 走可测内核 `is_home_root_with`,不碰进程级 `KANZEI_HOME`(与 home.rs 的顺序
    /// 测试并行跑会互踩环境变量)。
    #[test]
    fn kanzei_home指向项目根或其kanzei时被拦() {
        let root = project_root_fixture("kanzei-home-collide");
        let proj = root.join("home").join("projects").join("repo");
        // 场景 A:KANZEI_HOME 指到项目自己的 .kanzei(root=/proj,kh=/proj/.kanzei)。
        let kh_at_kanzei = proj.join(".kanzei");
        std::fs::create_dir_all(&kh_at_kanzei).unwrap();
        assert!(
            is_home_root_with(&proj, None, Some(&kh_at_kanzei)),
            "KANZEI_HOME 指到项目自己的 .kanzei 必须被拦(缺口③)"
        );
        // 场景 B:全局根就是项目根本身。
        assert!(is_home_root_with(&proj, None, Some(&proj)));
        // 场景 C:全局根在别处(正常形态),项目根不该被误拦。
        let other_kh = root.join("home").join(".kanzei");
        assert!(
            !is_home_root_with(&proj, None, Some(&other_kh)),
            "全局根在别处时正常项目不该被拦"
        );
        // 场景 D:真实 HOME 语义不受影响(home 参数仍优先认)。
        assert!(is_home_root_with(&proj, Some(&proj), None));
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
            // canonicalize 的产物形态:剥了 `\\?\` 才认得出来。
            assert!(is_home_root(&PathBuf::from(format!(
                r"\\?\{}",
                home.display()
            ))));
        }
        assert!(!is_home_root(&home.join("projects")));
    }

    /// D-194 补漏:`dir_key` 不折叠 `.` / `..` 时,`C:\Users\kanzei\.` 这类写法让 HOME
    /// 拦截静默失效——而 `resolve_project_root` 的标记校验对它照样成立(HOME 下有
    /// `.kanzei`),两道拦截一起被绕过,project 级 state.db 被写进全局配置根 `~/.kanzei`
    /// (实测发生过)。
    ///
    /// 这条路是 R-182 的显式主根入口打开的:在那之前根恒来自 `current_dir()`,不含
    /// `.`/`..` 段,写不出这种串;新入口收的正是用户任意书写的路径。
    #[test]
    fn is_home_root_folds_dot_and_dotdot_segments() {
        let Some(home) = dirs::home_dir() else {
            return; // 无 HOME 的环境跳过,不是被测行为。
        };
        let sep = std::path::MAIN_SEPARATOR;
        let text = home.display().to_string();
        let mut forms = vec![
            // 尾随 `.`:文件系统里就是 HOME 自己。
            PathBuf::from(format!("{text}{sep}.")),
            // 下一级再 `..` 弹回来。折叠是纯词法的,所以那一级存不存在都一样。
            PathBuf::from(format!("{text}{sep}Documents{sep}..")),
            // `.` 后面还跟着尾分隔符。
            PathBuf::from(format!("{text}{sep}.{sep}")),
            // 多段叠加,一路弹回 HOME。
            PathBuf::from(format!("{text}{sep}a{sep}..{sep}.{sep}b{sep}..")),
        ];
        #[cfg(windows)]
        {
            let slash = text.replace('\\', "/");
            forms.push(PathBuf::from(format!("{slash}/./")));
            forms.push(PathBuf::from(format!("{slash}/Documents/..")));
            // 大小写 + `.` 段 + `\\?\` 前缀三者叠加,任一环节漏了都拦不住。
            forms.push(PathBuf::from(format!(
                r"\\?\{}\.",
                text.to_lowercase().trim_end_matches('\\')
            )));
        }
        for form in forms {
            assert!(
                is_home_root(&form),
                "含 . / .. 的写法必须被认成 HOME: {}",
                form.display()
            );
        }
    }

    /// 折叠不许过头:路径里带 `.` 的**合法目录名**(`v1.0`、`.config`)不是 `.` 段,
    /// 正常项目根不能被误拦;`..` 也必须真的向上一级,而不是被吞掉。
    #[test]
    fn dir_key_keeps_dotted_directory_names() {
        let app = PathBuf::from(r"C:\proj\v1.0\app");
        // `v1.0` / `.config` 是目录名,不是 `.` 段:各自都还在。
        assert_ne!(dir_key(&app), dir_key(&PathBuf::from(r"C:\proj\v1.0")));
        assert_ne!(
            dir_key(&app),
            dir_key(&PathBuf::from(r"C:\proj\v1.0\app\.config"))
        );
        assert_ne!(dir_key(&app), dir_key(&PathBuf::from(r"C:\proj\app")));
        // 而真正的 `.` 段确实被折掉。
        assert_eq!(
            dir_key(&app),
            dir_key(&PathBuf::from(r"C:\proj\v1.0\app\."))
        );
        assert_eq!(
            dir_key(&app),
            dir_key(&PathBuf::from(r"C:\proj\v1.0\app\sub\.."))
        );

        let Some(home) = dirs::home_dir() else {
            return;
        };
        // 正常项目根(哪怕就在 HOME 底下、哪怕名字里带点)不被误判成 HOME。
        assert!(!is_home_root(&home.join("proj").join("v1.0").join("app")));
        assert!(!is_home_root(&home.join("v1.0")));
        assert!(!is_home_root(&home.join(".config")));
        // `..` 真的向上一级:HOME 的父目录不是 HOME。
        assert!(!is_home_root(&home.join("..")));
        assert!(!is_home_root(&home.join("a").join("..").join("..")));
    }

    /// 造一个空的临时夹具根,名字带 pid + 纳秒,避免同进程并发用例互踩。
    fn 临时夹具根(标签: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kanzei-{标签}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    /// `C:\x\y` → `\\localhost\C$\x\y`(本机管理共享)。拿不到盘符就返回 None。
    #[cfg(windows)]
    fn 管理共享形态(path: &Path, host: &str) -> Option<PathBuf> {
        let text = path.display().to_string();
        let bytes = text.as_bytes();
        if bytes.len() < 3 || bytes[1] != b':' || !bytes[0].is_ascii_alphabetic() {
            return None;
        }
        Some(PathBuf::from(format!(
            r"\\{host}\{}${}",
            &text[..1],
            &text[2..]
        )))
    }

    /// D-194 补漏三(对抗复核实测推翻了词法折叠):**「词法不同、文件系统同一」的别名补不完**。
    ///
    /// 复核实测出的两条现成绕过:`C:\Users\kanzei.`(Windows 剥掉末段尾随点)与
    /// `\\localhost\C$\Users\kanzei`(UNC,漫游/网络 profile 下本来就是合法写法);
    /// 再往下还有符号链接、junction、`subst` 虚拟盘、8.3 短名。本条用**真夹具**证明
    /// 身份层认得出它们,并且**每条都先断言词法层认不出**——否则用例证明不了身份层在干活。
    ///
    /// 夹具全部造在系统临时目录,一个字节都不碰真 HOME;造不出来的形态跳过并打印原因。
    #[test]
    #[cfg(windows)]
    fn 文件系统别名一律被认成同一个目录() {
        let root = 临时夹具根("d194-alias");
        // 名字取长一点,8.3 短名才有 `~1` 形态可测。
        let 目标 = root.join("homedirectory");
        std::fs::create_dir_all(目标.join(".kanzei")).unwrap();
        // 对照组:名字里带点的**合法**项目根,不能被误拦。
        let 带点项目根 = root.join("v1.0").join("app");
        std::fs::create_dir_all(&带点项目根).unwrap();

        let mut 跳过: Vec<String> = Vec::new();
        let 断言别名 = |形态: &Path, 名称: &str| {
            assert_ne!(
                dir_key(&目标),
                dir_key(形态),
                "夹具失效:{名称} 在词法上必须与目标不同,否则这条用例证明不了身份层"
            );
            assert!(
                is_same_dir(&目标, 形态),
                "{名称} 在磁盘上就是同一个目录,必须被认出来: {}",
                形态.display()
            );
        };

        // ① 尾随点:Windows 剥掉末段的尾随点,`homedirectory.` 就是 `homedirectory`。
        断言别名(&PathBuf::from(format!("{}.", 目标.display())), "尾随点");

        // ② junction:不需要管理员权限,mklink /J 即可。
        let junction = root.join("junc");
        let 造junction = std::process::Command::new("cmd")
            .args([
                "/c",
                "mklink",
                "/J",
                &junction.display().to_string(),
                &目标.display().to_string(),
            ])
            .output();
        match 造junction {
            Ok(_) if junction.is_dir() => 断言别名(&junction, "junction"),
            Ok(out) => 跳过.push(format!(
                "junction: mklink /J 没造出来({})",
                String::from_utf8_lossy(&out.stdout).trim()
            )),
            Err(e) => 跳过.push(format!("junction: 起不了 cmd({e})")),
        }

        // ③ 符号链接:Windows 上要管理员或开发者模式,普通会话大概率造不出来。
        let 符号链接 = root.join("symd");
        match std::os::windows::fs::symlink_dir(&目标, &符号链接) {
            Ok(()) => 断言别名(&符号链接, "符号链接"),
            Err(e) => 跳过.push(format!(
                "符号链接: {e}(Windows 建目录符号链接需管理员或开发者模式)"
            )),
        }

        // ④ 8.3 短名:卷上可能已经关掉 8dot3name 生成。
        let 短名 = root.join("HOMEDI~1");
        if 短名.is_dir() {
            断言别名(&短名, "8.3 短名");
        } else {
            跳过.push("8.3 短名: 本卷未生成 8dot3 名(fsutil 8dot3name 已关)".into());
        }

        // ⑤ UNC 管理共享:两种主机写法各一遍;非管理员会话访问 C$ 会失败,失败即跳过。
        for host in ["localhost", "127.0.0.1"] {
            match 管理共享形态(&目标, host) {
                Some(unc) if unc.is_dir() => 断言别名(&unc, &format!(r"UNC \\{host}\C$")),
                Some(unc) => 跳过.push(format!("UNC {}: 不可达(需要管理共享权限)", unc.display())),
                None => 跳过.push(format!("UNC {host}: 夹具不在盘符路径上")),
            }
        }

        // 反向:别名判定不许过头——不同的目录仍然是不同的目录。
        assert!(!is_same_dir(&目标, &带点项目根), "带点的合法项目根被误拦");
        assert!(!is_same_dir(&目标, &root.join("v1.0")));
        assert!(!is_same_dir(&目标, &root), "父目录不是同一个目录");
        assert!(
            !is_same_dir(&目标, &目标.join(".kanzei")),
            "子目录不是同一个目录"
        );

        if !跳过.is_empty() {
            eprintln!("文件系统别名用例跳过的形态: {跳过:#?}");
        }
        // junction 先单独删,避免递归删除时穿到目标里去。
        let _ = std::fs::remove_dir(&junction);
        let _ = std::fs::remove_dir(&符号链接);
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// 复核实测报上来的那两条绕过,直接打在**真 HOME**上(只读:比较不写任何东西)。
    /// 这是红线用例——它红就意味着 `KANZEI_PROJECT_ROOT` 又能把项目产物写进 `~/.kanzei`。
    #[test]
    #[cfg(windows)]
    fn 真home的尾随点与unc写法不再绕过拦截() {
        let Some(home) = dirs::home_dir() else {
            return; // 无 HOME 的环境跳过,不是被测行为。
        };
        let 尾随点 = PathBuf::from(format!("{}.", home.display()));
        assert_ne!(dir_key(&home), dir_key(&尾随点), "夹具失效:词法上本该不同");
        assert!(
            is_home_root(&尾随点),
            "`{}` 在磁盘上就是 HOME,必须被拦下",
            尾随点.display()
        );

        let mut 测到的unc = 0usize;
        for host in ["localhost", "127.0.0.1"] {
            let Some(unc) = 管理共享形态(&home, host) else {
                continue;
            };
            if !unc.is_dir() {
                eprintln!("跳过 {}(需要管理共享权限)", unc.display());
                continue;
            }
            assert_ne!(dir_key(&home), dir_key(&unc), "夹具失效:词法上本该不同");
            assert!(is_home_root(&unc), "UNC 写法必须被拦下: {}", unc.display());
            测到的unc += 1;
        }
        if 测到的unc == 0 {
            eprintln!("本机管理共享不可达,UNC 形态未覆盖(见同名夹具用例)");
        }
    }

    /// 拿不到文件系统身份时**回落词法折叠**,不因为 canonicalize 失败就当成两个目录。
    /// (路径不存在这条在 CLI 里其实到不了 `is_home_root`——`resolve_project_root`
    /// 先报"显式主根指向的路径不存在";这里钉的是守卫本身的兜底行为。)
    #[test]
    fn canonicalize失败时回落到词法折叠() {
        let 不存在 = std::env::temp_dir().join(format!(
            "kanzei-d194-missing-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        assert!(!不存在.exists());
        assert!(std::fs::canonicalize(&不存在).is_err());
        assert!(is_same_dir(&不存在, &不存在.join(".")));
        assert!(is_same_dir(&不存在, &不存在.join("x").join("..")));
        assert!(!is_same_dir(&不存在, &不存在.join("x")));
    }

    /// R-182 内容②:`load_at_root` 是**显式**入口——给它哪个根就读哪个根,
    /// 一步都不许向上发现;发现式的老入口 `load_with_warnings` 语义原样不变。
    #[test]
    fn load_at_root不做根发现() {
        let root = std::env::temp_dir().join(format!(
            "kanzei-r182-at-root-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let sub = root.join("sub");
        std::fs::create_dir_all(root.join(".kanzei")).unwrap();
        std::fs::create_dir_all(sub.join(".kanzei")).unwrap();
        std::fs::write(
            root.join(".kanzei").join("kanzei.toml"),
            "[models]\nprimary = \"mock:root-model\"\n",
        )
        .unwrap();
        std::fs::write(
            sub.join(".kanzei").join("kanzei.toml"),
            "[models]\nprimary = \"mock:sub-model\"\n",
        )
        .unwrap();

        // 显式入口:传 root 就读 root 那份,哪怕 cwd 概念上在 sub 里。
        let (at_root, _) = KanzeiConfig::load_with_warnings_at_root(&root).unwrap();
        assert_eq!(at_root.models.primary.as_deref(), Some("mock:root-model"));
        assert_eq!(
            KanzeiConfig::load_at_root(&root)
                .unwrap()
                .models
                .primary
                .as_deref(),
            Some("mock:root-model")
        );
        // 发现式老入口:从 sub 出发仍然命中 sub 自己的 `.kanzei`(行为一字不改)。
        let (discovered, _) = KanzeiConfig::load_with_warnings(&sub).unwrap();
        assert_eq!(discovered.models.primary.as_deref(), Some("mock:sub-model"));

        std::fs::remove_dir_all(root).unwrap();
    }

    /// 优先级定死:参数 > 环境变量 > 发现式。本函数只看「显式还是没有」这一层;
    /// 参数与环境变量的先后由 CLI 侧合成(main.rs 的 `explicit_main_root`)。
    #[test]
    fn resolve_project_root显式优先() {
        let root = std::env::temp_dir().join(format!(
            "kanzei-r182-resolve-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let sub = root.join("sub");
        std::fs::create_dir_all(root.join(".kanzei")).unwrap();
        std::fs::create_dir_all(sub.join(".kanzei")).unwrap();

        // 显式给了根:cwd 在 sub 里也照样返回 root。
        assert_eq!(
            resolve_project_root(Some(&root), &sub).unwrap(),
            root.clone()
        );
        // 没给:逐字节退回 discover_project_root——这同时证明本批没去改它。
        assert_eq!(
            resolve_project_root(None, &sub).unwrap(),
            discover_project_root(&sub).unwrap()
        );
        assert_eq!(resolve_project_root(None, &sub).unwrap(), sub.clone());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn 显式主根必须是真项目根() {
        let root = std::env::temp_dir().join(format!(
            "kanzei-r182-marker-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let empty = root.join("empty");
        let file = root.join("a-file.txt");
        let worktree = root.join("worktree");
        std::fs::create_dir_all(&empty).unwrap();
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(&file, "not a directory").unwrap();
        // worktree 的 `.git` 是文件,必须照样算标记。
        std::fs::write(worktree.join(".git"), "gitdir: ../repo/.git/worktrees/w\n").unwrap();

        for bad in [root.join("does-not-exist"), empty.clone(), file.clone()] {
            let error = resolve_project_root(Some(&bad), &root)
                .unwrap_err()
                .to_string();
            // 错误必须点名来源键名,否则用户不知道该去改哪个开关/变量。
            assert!(
                error.contains("--project-root") && error.contains("KANZEI_PROJECT_ROOT"),
                "错误文本要点名来源: {error}"
            );
        }
        assert_eq!(
            resolve_project_root(Some(&worktree), &root).unwrap(),
            worktree
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    /// 不做 canonicalize:`\\?\` 形态会让用户已写的绝对路径权限规则一夜失配。
    #[test]
    fn 显式主根不做canonicalize() {
        let root = std::env::temp_dir().join(format!(
            "kanzei-r182-nocanon-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".kanzei")).unwrap();

        let mut forms = vec![PathBuf::from(format!(
            "{}{}",
            root.display(),
            std::path::MAIN_SEPARATOR
        ))];
        #[cfg(windows)]
        forms.push(PathBuf::from(
            root.display().to_string().to_lowercase().replace('\\', "/"),
        ));
        for form in forms {
            let resolved = resolve_project_root(Some(&form), &root).unwrap();
            assert!(
                !resolved.display().to_string().starts_with(r"\\?\"),
                "不该 canonicalize: {}",
                resolved.display()
            );
            // 原样返回:用户写下什么就是什么。
            assert_eq!(resolved, form);
        }

        std::fs::remove_dir_all(root).unwrap();
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

    // D-300:barrier_timeout_secs 曾同时漏接 overlay 与 unknown_keys 名单——项目层设了
    // 静默不生效,还被误报「未知配置项」;既有 unknown_keys_schema_matches_struct 对
    // [limits] 是盲区(None 字段不进序列化)。本测试穷举 Limits 全字段:
    // ①每个字段都显式出现在 TOML 里(新增字段没进本清单即红,编译期由 serde 字段名驱动);
    // ②unknown_keys 零告警(名单漏键即红);③merge 后逐字段等于层值(overlay 漏键即红)。
    #[test]
    fn limits_全字段_层叠往返不丢值_且名单穷举() {
        let layer_toml = r#"
[limits]
max_tokens = 1
subagent_max_tokens = 2
subagent_timeout_secs = 3
barrier_timeout_secs = 4
context_budget_ratio = 0.5
recent_verbatim_ratio = 0.25
max_tasks_per_turn = 5
max_parallel_tools = 6
transport_retries = 7
rate_limit_retries = 8
stream_restarts = 9
compact_buffer_tokens = 10
prune_protect_tokens = 11
prune_min_gain_tokens = 12
"#;
        // ①穷举完整性:结构体的每个字段都必须在上面的 TOML 里显式赋了值。
        let layer: KanzeiConfig = toml::from_str(layer_toml).unwrap();
        let layer_json = serde_json::to_value(&layer.limits).unwrap();
        for (key, value) in layer_json.as_object().unwrap() {
            assert!(
                !value.is_null(),
                "Limits 新增字段 `{key}` 没进本测试的 TOML——补进来,并同步 overlay! 宏与 unknown_keys 名单"
            );
        }
        // ②unknown_keys 名单:全部键都该被 schema 认识,不得误报。
        let raw: toml::Value = toml::from_str(layer_toml).unwrap();
        assert_eq!(unknown_keys(&raw), Vec::<String>::new());
        // ③merge 层叠:项目层写的每个键都要活着到达运行时。
        let mut base = KanzeiConfig::default();
        merge(&mut base, layer);
        assert_eq!(serde_json::to_value(&base.limits).unwrap(), layer_json);
    }

    // D-245 验收②:merge_file 必须把 [cadence] 的显式键逐项覆盖进 KanzeiConfig。
    // 此前 merge() 没有 cadence 分支,文件里写了也到不了运行时——config.cadence
    // 恒为默认(复现实证:全仓 grep 除 settings/config 定义外零消费方)。
    #[test]
    fn cadence_层叠合并_显式键覆盖_缺键保持全局() {
        // 全局层:full_test=release_only + push=per_entry(显式写 per_entry)。
        let global = temp_config_dir();
        std::fs::write(
            global.join("kanzei.toml"),
            "[cadence]\nfull_test = \"release_only\"\npush = \"per_entry\"\n",
        )
        .unwrap();
        // 项目层:只显式写 full_test=every_commit,其余键不得被打回默认。
        let project = temp_config_dir();
        std::fs::create_dir_all(project.join(".kanzei")).unwrap();
        std::fs::write(
            project.join(".kanzei").join("kanzei.toml"),
            "[cadence]\nfull_test = \"every_commit\"\n",
        )
        .unwrap();
        let config = load_two_layer(&global, &project);
        // 项目层覆盖了 full_test。
        assert_eq!(config.cadence.full_test, FullTestCadence::EveryCommit);
        // 全局层的 push=per_entry 保持——项目层没写 push,不得被默认值(per_batch)覆盖。
        assert_eq!(config.cadence.push, PushCadence::PerEntry);
        // 全局层的默认仍生效:targeted_test 没在任一层显式写 → §1.4 默认。
        assert_eq!(
            config.cadence.targeted_test,
            TargetedTestCadence::EveryCommit
        );
        std::fs::remove_dir_all(global).ok();
        std::fs::remove_dir_all(project).ok();
    }

    fn temp_config_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-config-cadence-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn load_two_layer(global: &std::path::Path, project: &std::path::Path) -> KanzeiConfig {
        // 模拟 load_with_warnings_at_root 的全局+项目层叠,但两个目录都显式可控。
        let mut config = KanzeiConfig::default();
        let mut warnings = Vec::new();
        merge_file(&mut config, &global.join("kanzei.toml"), &mut warnings).unwrap();
        merge_file(
            &mut config,
            &project.join(".kanzei").join("kanzei.toml"),
            &mut warnings,
        )
        .unwrap();
        config
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

    // ══ R-183 内容①:非交互三态策略解析与 fail-closed(验收②)══

    #[test]
    fn non_interactive_三态解析() {
        use crate::config::NonInteractive;
        assert_eq!(NonInteractive::parse("deny"), Some(NonInteractive::Deny));
        assert_eq!(
            NonInteractive::parse("rules_only"),
            Some(NonInteractive::RulesOnly)
        );
        assert_eq!(
            NonInteractive::parse("allow_listed"),
            Some(NonInteractive::AllowListed)
        );
        // 大小写与首尾空白宽容。
        assert_eq!(
            NonInteractive::parse("  Rules_Only "),
            Some(NonInteractive::RulesOnly)
        );
    }

    #[test]
    fn non_interactive_缺省与非法取值_fail_closed回落deny() {
        // 缺键、空串、无法识别的取值一律回落 Deny——旧配置逐字节不变(验收②)。
        let empty: KanzeiConfig = toml::from_str("").unwrap();
        assert_eq!(
            empty.non_interactive_policy(),
            crate::config::NonInteractive::Deny
        );
        let blank: KanzeiConfig =
            toml::from_str("[permissions]\nnon_interactive = \"\"\n").unwrap();
        assert_eq!(
            blank.non_interactive_policy(),
            crate::config::NonInteractive::Deny
        );
        let junk: KanzeiConfig =
            toml::from_str("[permissions]\nnon_interactive = \"bogus\"\n").unwrap();
        assert_eq!(
            junk.non_interactive_policy(),
            crate::config::NonInteractive::Deny
        );
        // 非法取值必须给告警,否则用户以为开了 rules_only 实际每次停机还归不到因。
        assert!(junk.non_interactive_policy_warning().is_some());
        assert!(empty.non_interactive_policy_warning().is_none());
        let rules: KanzeiConfig =
            toml::from_str("[permissions]\nnon_interactive = \"rules_only\"\n").unwrap();
        assert_eq!(
            rules.non_interactive_policy(),
            crate::config::NonInteractive::RulesOnly
        );
        assert!(rules.non_interactive_policy_warning().is_none());
    }
}
