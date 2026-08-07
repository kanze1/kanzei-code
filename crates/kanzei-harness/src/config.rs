//! kanzei.toml:全局(~/.kanzei/)→ 项目(.kanzei/,从 cwd 向上发现),后者覆盖前者。
//! 配置本身以组件形式进入 harness(贡献权限规则),没有第二条 config→runtime 路径。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::permission::Rule;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct ModelRoles {
    pub primary: Option<String>,
    pub fast: Option<String>,
    /// 思考强度默认档:"off"(默认)| "low" | "medium" | "high"。
    /// 运行时可被桌面端的每进程选择覆盖;未配置时保持 off,行为与既有一致。
    #[serde(default)]
    pub reasoning: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct ProfileSection {
    pub default: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
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
    /// 全局 + 项目层叠加载。任一文件解析失败返回错误(配置错误要炸在启动,不能静默)。
    pub fn load(cwd: &Path) -> anyhow::Result<KanzeiConfig> {
        let mut config = KanzeiConfig::default();
        if let Some(home) = dirs::home_dir() {
            merge_file(&mut config, &home.join(".kanzei").join("kanzei.toml"))?;
        }
        if let Some(project) = discover_project_config(cwd) {
            merge_file(&mut config, &project)?;
        }
        config.fill_defaults();
        Ok(config)
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

fn merge_file(config: &mut KanzeiConfig, path: &Path) -> anyhow::Result<()> {
    if !path.is_file() {
        return Ok(());
    }
    let text = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", path.display()))?;
    let layer: KanzeiConfig = toml::from_str(&text)
        .map_err(|e| anyhow::anyhow!("invalid config {}: {e}", path.display()))?;
    merge(config, layer);
    Ok(())
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
pub fn append_allow_rule(
    project_root: &Path,
    action: &str,
    resource: &str,
) -> anyhow::Result<PathBuf> {
    let path = project_root.join(".kanzei").join("kanzei.toml");
    let mut config = if path.is_file() {
        let text = std::fs::read_to_string(&path)?;
        toml::from_str(&text).map_err(|e| anyhow::anyhow!("invalid {}: {e}", path.display()))?
    } else {
        KanzeiConfig::default()
    };
    config.permissions.rules.push(Rule {
        action: action.to_string(),
        resource: resource.to_string(),
        effect: crate::permission::Effect::Allow,
    });
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, toml::to_string_pretty(&config)?)?;
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
