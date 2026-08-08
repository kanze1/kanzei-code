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
    // reasoning 曾被漏掉:项目级设的思考强度会被静默忽略,而 primary/fast 却生效,
    // 表现为"同一份配置里有的键管用有的不管用"——最难查的那种。
    if layer.models.reasoning.is_some() {
        base.models.reasoning = layer.models.reasoning;
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
        assert!(warnings[1].contains("不生效"), "应说明 legacy 被覆盖: {warnings:?}");
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
        assert_eq!(base.models.reasoning.as_deref(), Some("high"), "项目级思考强度未生效");
        // 层里没设的键不得被清空——空 [models] 表不该抹掉全局配置。
        assert_eq!(base.models.primary.as_deref(), Some("anthropic:claude-sonnet-5"));
        assert_eq!(base.models.fast.as_deref(), Some("ollama:qwen3"));
    }
}
