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
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ModelRoles {
    pub primary: Option<String>,
    pub fast: Option<String>,
    /// 思考强度默认档:"off"(默认)| "low" | "medium" | "high"。
    /// 运行时可被桌面端的每进程选择覆盖;未配置时保持 off,行为与既有一致。
    #[serde(default)]
    pub reasoning: Option<String>,
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
        if let Some(home) = dirs::home_dir() {
            merge_file(&mut config, &home.join(".kanzei").join("kanzei.toml"), &mut warnings)?;
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
        if self.models.primary.is_none() {
            self.models.primary = Some("anthropic:claude-sonnet-5".into());
        }
        if self.models.fast.is_none() {
            self.models.fast = Some("ollama:qwen3.5:4b".into());
        }
    }

    /// "primary"/"fast"(角色)或 "provider:model"(直指)→ ResolvedModel。
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
        &["models", "providers", "proxy", "profile", "permissions"],
        &mut out,
    );
    if let Some(models) = value.get("models") {
        check(models, "models", &["primary", "fast", "reasoning"], &mut out);
    }
    if let Some(providers) = value.get("providers").and_then(|p| p.as_table()) {
        for (name, provider) in providers {
            check(
                provider,
                &format!("providers.{name}"),
                &["protocol", "base_url", "api_key_env", "api_key", "auth", "context_limit"],
                &mut out,
            );
        }
    }
    if let Some(profile) = value.get("profile") {
        check(profile, "profile", &["default"], &mut out);
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
    base.providers.extend(layer.providers);
    if layer.proxy.is_some() {
        base.proxy = layer.proxy;
    }
    if layer.profile.default.is_some() {
        base.profile.default = layer.profile.default;
    }
    base.permissions.rules.extend(layer.permissions.rules);
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
    let rules = permissions.entry("rules").or_insert(toml_edit::Item::ArrayOfTables(
        toml_edit::ArrayOfTables::new(),
    ));
    let Some(rules) = rules.as_array_of_tables_mut() else {
        anyhow::bail!("{}: `permissions.rules` 不是数组表,无法追加规则", path.display());
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

/// 项目根 = 向上最近的含 `.kanzei/` 或 `.git/` 的目录;都没有则 cwd 本身。
pub fn discover_project_root(cwd: &Path) -> Option<PathBuf> {
    let mut dir = Some(cwd);
    let mut fallback = None;
    while let Some(d) = dir {
        if d.join(".kanzei").is_dir() {
            return Some(d.to_path_buf());
        }
        if fallback.is_none() && d.join(".git").is_dir() {
            fallback = Some(d.to_path_buf());
        }
        dir = d.parent();
    }
    fallback.or_else(|| Some(cwd.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_model_resolution() {
        let mut c = KanzeiConfig::default();
        c.fill_defaults();
        let m = c.resolve_model("primary").unwrap();
        assert_eq!(m.provider_name, "anthropic");
        assert_eq!(m.model, "claude-sonnet-5");
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
    fn bash_always_allow_keeps_exact_command() {
        assert_eq!(generalize_resource("bash", "git status"), "git status");
        assert_eq!(generalize_resource("write", "notes.md"), "notes.md");
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
        assert_eq!(config.models.primary.as_deref(), Some("anthropic:claude-sonnet-5"));
        assert!(config.providers.contains_key("kimi"));
        let raw: toml::Value = toml::from_str(text).unwrap();
        let mut unknown = unknown_keys(&raw);
        unknown.sort();
        assert_eq!(
            unknown,
            vec!["future_section", "models.new_feature_field", "providers.kimi.typo_fielt"]
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
        let raw: toml::Value =
            toml::from_str(&toml::to_string_pretty(&config).unwrap()).unwrap();
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
            assert!(saved.contains(expected), "missing preserved text: {expected}\n---\n{saved}");
        }
        let config: KanzeiConfig = toml::from_str(&saved).unwrap();
        assert_eq!(config.permissions.rules.len(), 1);
        assert_eq!(config.permissions.rules[0].resource, "git status");
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
        let resource = r#"{"command":"git status > .kanzei/project/requirements.md","workdir":"C:/project"}"#;
        let path = append_allow_rule(&root, "bash", resource).unwrap();
        let saved: KanzeiConfig = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(saved.permissions.rules.len(), 1);
        assert_eq!(saved.permissions.rules[0].action, "bash");
        assert_eq!(saved.permissions.rules[0].resource, resource);
        assert_eq!(saved.permissions.rules[0].effect, crate::permission::Effect::Allow);
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
        assert_eq!(legacy.len(), 1);
        assert_eq!(legacy[0].resource, "git status");
        assert_eq!(config.permissions.rules.len(), 3);
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
}
