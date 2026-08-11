//! Settings command boundary and global configuration commands.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::json;
use tauri::State;

use crate::AppState;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SettingsPayload {
    pub(crate) primary: String,
    pub(crate) fast: String,
    pub(crate) proxy: String,
    #[serde(default)]
    pub(crate) reasoning: Option<String>,
    #[serde(default)]
    pub(crate) codex_fast_mode: bool,
    pub(crate) profile_default: Option<String>,
    #[serde(default)]
    pub(crate) profile: Option<String>,
    pub(crate) providers: Vec<ProviderPayload>,
    #[serde(default)]
    pub(crate) limits: LimitsPayload,
    #[serde(default)]
    pub(crate) cadence: Option<CadencePayload>,
}

/// 设置页节奏表单载荷:留空(None)即"用内置默认",保存时该键从 [cadence] 移除,
/// 回落 serde default(conventions §1.4 当前值)。键名与 settings_get 返回的
/// Cadence 序列化一致(snake_case),前后端只此一份映射。
#[derive(Deserialize, Default)]
pub(crate) struct CadencePayload {
    #[serde(default)]
    pub(crate) full_test: Option<String>,
    #[serde(default)]
    pub(crate) full_test_batches: Option<u32>,
    #[serde(default)]
    pub(crate) targeted_test: Option<String>,
    #[serde(default)]
    pub(crate) commit: Option<String>,
    #[serde(default)]
    pub(crate) push: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LimitsPayload {
    #[serde(default)]
    pub(crate) max_tokens: Option<u32>,
    #[serde(default)]
    pub(crate) subagent_max_tokens: Option<u32>,
    #[serde(default)]
    pub(crate) subagent_timeout_secs: Option<u64>,
    #[serde(default)]
    pub(crate) context_budget_ratio: Option<f64>,
    #[serde(default)]
    pub(crate) recent_verbatim_ratio: Option<f64>,
    #[serde(default)]
    pub(crate) max_tasks_per_turn: Option<usize>,
    #[serde(default)]
    pub(crate) max_parallel_tools: Option<usize>,
    #[serde(default)]
    pub(crate) transport_retries: Option<u32>,
    #[serde(default)]
    pub(crate) rate_limit_retries: Option<u32>,
    #[serde(default)]
    pub(crate) stream_restarts: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderPayload {
    pub(crate) name: String,
    pub(crate) protocol: String,
    pub(crate) base_url: String,
    pub(crate) api_key_env: Option<String>,
    #[serde(default)]
    pub(crate) api_key: Option<String>,
    #[serde(default)]
    pub(crate) auth: Option<String>,
    #[serde(default)]
    pub(crate) context_limit: Option<u64>,
}

pub(crate) fn global_config_path() -> PathBuf {
    kanzei_harness::kanzei_home()
        .unwrap_or_default()
        .join("kanzei.toml")
}

/// 设置标量并保留原值上的空白/行尾注释装饰。
pub(crate) fn settings_set_value(
    table: &mut toml_edit::Table,
    key: &str,
    value: impl Into<toml_edit::Value>,
) {
    let mut value: toml_edit::Value = value.into();
    if let Some(existing) = table.get(key).and_then(|item| item.as_value()) {
        *value.decor_mut() = existing.decor().clone();
    }
    table[key] = toml_edit::Item::Value(value);
}

pub(crate) fn settings_set_or_remove(
    table: &mut toml_edit::Table,
    key: &str,
    value: Option<String>,
) {
    match value {
        Some(v) => settings_set_value(table, key, v),
        None => {
            table.remove(key);
        }
    }
}

pub(crate) fn settings_set_or_reset(
    table: &mut toml_edit::Table,
    key: &str,
    value: Option<String>,
    default_value: &str,
) {
    match value {
        Some(v) => settings_set_value(table, key, v),
        None if table.contains_key(key) => {
            settings_set_value(table, key, default_value.to_string())
        }
        None => {}
    }
}

pub(crate) fn settings_set_or_remove_num(
    table: &mut toml_edit::Table,
    key: &str,
    value: Option<impl Into<toml_edit::Value>>,
) {
    match value {
        Some(v) => settings_set_value(table, key, v),
        None => {
            table.remove(key);
        }
    }
}

pub(crate) fn settings_table<'a>(
    doc: &'a mut toml_edit::DocumentMut,
    name: &str,
) -> Result<&'a mut toml_edit::Table, String> {
    doc.entry(name)
        .or_insert(toml_edit::table())
        .as_table_mut()
        .ok_or_else(|| format!("配置节 `{name}` 不是表,无法保存设置"))
}

/// 保存前校验模型角色:`provider:model` 里的 provider 必须确实配了。
pub(crate) fn validate_model_roles(payload: &SettingsPayload) -> Result<(), String> {
    let mut probe = kanzei_harness::KanzeiConfig::default();
    for p in &payload.providers {
        probe.providers.insert(
            p.name.trim().to_string(),
            kanzei_harness::config::ProviderConfig {
                protocol: p.protocol.clone(),
                base_url: p.base_url.clone(),
                api_key_env: p.api_key_env.clone(),
                api_key: p.api_key.clone(),
                auth: p.auth.clone(),
                context_limit: p.context_limit,
            },
        );
    }
    probe.fill_defaults();
    for (role, value) in [("primary", &payload.primary), ("fast", &payload.fast)] {
        let spec = value.trim();
        if spec.is_empty() {
            continue;
        }
        probe.resolve_model(spec).map_err(|e| format!(
            "{role} 的模型 `{spec}` 无法解析:{e}。格式应为 provider:model,且 provider 必须是下面 Provider 表里的名称。"
        ))?;
    }
    Ok(())
}

pub(crate) fn settings_read_document(path: &Path) -> Result<toml_edit::DocumentMut, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("读取配置失败 {}: {e}", path.display())),
    };
    toml::from_str::<kanzei_harness::KanzeiConfig>(&text)
        .map_err(|e| format!("现有配置无法解析,拒绝覆盖保存 {}: {e}", path.display()))?;
    text.parse()
        .map_err(|e| format!("现有配置无法解析,拒绝覆盖保存 {}: {e}", path.display()))
}

pub(crate) fn settings_write_document(
    doc: toml_edit::DocumentMut,
    path: &Path,
) -> Result<(), String> {
    let text = doc.to_string();
    toml::from_str::<kanzei_harness::KanzeiConfig>(&text)
        .map_err(|e| format!("保存结果自校验失败,已放弃写入: {e}"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, text).map_err(|e| e.to_string())
}

pub(crate) fn settings_apply_scalar_fields(
    doc: &mut toml_edit::DocumentMut,
    payload: &SettingsPayload,
) -> Result<(), String> {
    settings_apply_model_fields(doc, payload)?;
    settings_set_or_reset(
        doc.as_table_mut(),
        "proxy",
        match payload.proxy.trim() {
            "" | "env" => None,
            other => Some(other.to_string()),
        },
        "env",
    );
    let profile = settings_table(doc, "profile")?;
    profile.set_implicit(true);
    // readonly 也是合法档位(defs.rs ProfileKind)。设置页的下拉只列 dev/research,
    // 但文件里写着 readonly 的用户不能因为路过一次设置页就被静默降级成 dev——
    // 白名单少一个值,表现出来就是"我明明没动这一项,保存后它自己变了"。
    settings_set_or_reset(
        profile,
        "default",
        payload
            .profile_default
            .as_ref()
            .or(payload.profile.as_ref())
            .filter(|p| matches!(p.as_str(), "dev" | "research" | "readonly"))
            .cloned(),
        "dev",
    );
    Ok(())
}

/// 只应用 [models] 节(primary/fast/reasoning/codex_fast_mode)。R-178 批4:D7
/// 作用域选择器第一版只覆盖这一节——写项目配置时不许带出 proxy/profile/limits/
/// cadence/providers,否则「选本项目保存」会悄悄把 provider 密钥写进被 git 跟踪的
/// 项目 toml(D7 边界明确排除)。
pub(crate) fn settings_apply_model_fields(
    doc: &mut toml_edit::DocumentMut,
    payload: &SettingsPayload,
) -> Result<(), String> {
    let models = settings_table(doc, "models")?;
    settings_set_or_remove(
        models,
        "primary",
        Some(payload.primary.trim().to_string()).filter(|s| !s.is_empty()),
    );
    settings_set_or_remove(
        models,
        "fast",
        Some(payload.fast.trim().to_string()).filter(|s| !s.is_empty()),
    );
    settings_set_or_reset(
        models,
        "reasoning",
        payload
            .reasoning
            .as_ref()
            .map(|v| v.trim().to_ascii_lowercase())
            .filter(|v| ["low", "medium", "high"].contains(&v.as_str())),
        "off",
    );
    settings_set_value(models, "codex_fast_mode", payload.codex_fast_mode);
    Ok(())
}

pub(crate) fn settings_apply_limits(
    doc: &mut toml_edit::DocumentMut,
    payload: &SettingsPayload,
) -> Result<(), String> {
    let limits = settings_table(doc, "limits")?;
    let l = &payload.limits;
    settings_set_or_remove_num(limits, "max_tokens", l.max_tokens.map(i64::from));
    settings_set_or_remove_num(
        limits,
        "subagent_max_tokens",
        l.subagent_max_tokens.map(i64::from),
    );
    settings_set_or_remove_num(
        limits,
        "subagent_timeout_secs",
        l.subagent_timeout_secs.map(|v| v as i64),
    );
    settings_set_or_remove_num(limits, "context_budget_ratio", l.context_budget_ratio);
    settings_set_or_remove_num(limits, "recent_verbatim_ratio", l.recent_verbatim_ratio);
    settings_set_or_remove_num(
        limits,
        "max_tasks_per_turn",
        l.max_tasks_per_turn.map(|v| v as i64),
    );
    settings_set_or_remove_num(
        limits,
        "max_parallel_tools",
        l.max_parallel_tools.map(|v| v as i64),
    );
    settings_set_or_remove_num(
        limits,
        "transport_retries",
        l.transport_retries.map(i64::from),
    );
    settings_set_or_remove_num(
        limits,
        "rate_limit_retries",
        l.rate_limit_retries.map(i64::from),
    );
    settings_set_or_remove_num(limits, "stream_restarts", l.stream_restarts.map(i64::from));
    if limits.is_empty() {
        doc.remove("limits");
    }
    Ok(())
}

/// [cadence] 节:载荷带节奏节时按表单落盘——枚举值按 serde snake_case 校名,
/// 非法值不写(配置保持干净,坏值交给加载侧报错);全空 = 清空既有键,回落
/// serde default(conventions §1.4 当前默认);载荷不带 cadence(旧前端)则整节不动。
pub(crate) fn settings_apply_cadence(
    doc: &mut toml_edit::DocumentMut,
    payload: &SettingsPayload,
) -> Result<(), String> {
    let Some(cadence) = &payload.cadence else {
        return Ok(());
    };
    let entries: [(&str, Option<&str>); 4] = [
        (
            "full_test",
            cadence.full_test.as_deref().filter(|v| {
                matches!(
                    *v,
                    "entry_close" | "every_commit" | "every_n_batches" | "release_only"
                )
            }),
        ),
        (
            "targeted_test",
            cadence
                .targeted_test
                .as_deref()
                .filter(|v| matches!(*v, "every_commit" | "off")),
        ),
        (
            "commit",
            cadence
                .commit
                .as_deref()
                .filter(|v| matches!(*v, "per_batch" | "per_entry")),
        ),
        (
            "push",
            cadence
                .push
                .as_deref()
                .filter(|v| matches!(*v, "per_entry" | "per_commit" | "periodic")),
        ),
    ];
    let table = settings_table(doc, "cadence")?;
    for (key, value) in entries {
        settings_set_or_remove(table, key, value.map(str::to_string));
    }
    settings_set_or_remove_num(
        table,
        "full_test_batches",
        cadence.full_test_batches.map(i64::from),
    );
    if table.is_empty() {
        doc.remove("cadence");
    }
    Ok(())
}

/// [providers] 节:载荷带非空 provider 清单 = **清单即权威**,配置里不在清单中的
/// `[providers.X]` 子表会被删除;载荷不带 provider(settings_open 的合成载荷、其他
/// 调用方)则整节不动,只增改不删——与 settings_apply_cadence「载荷不带 cadence 就整
/// 节不动」是同一条约定。
///
/// 加这条剪枝之前只有 upsert:设置页表格里点「×」删掉一个 provider,载荷里确实没它了,
/// 但配置里的子表从没被删过,重开设置页它又原样回来,用户看到的就是"删了没生效"。
/// 因此新调用方要么发整张表,要么一个 provider 都别发,不许发"只改动的那几行"。
pub(crate) fn settings_apply_providers(
    doc: &mut toml_edit::DocumentMut,
    payload: &SettingsPayload,
) -> Result<(), String> {
    let providers = settings_table(doc, "providers")?;
    providers.set_implicit(true);
    if !payload.providers.is_empty() {
        let keep: std::collections::BTreeSet<&str> = payload
            .providers
            .iter()
            .map(|p| p.name.trim())
            .filter(|name| !name.is_empty())
            .collect();
        let stale: Vec<String> = providers
            .iter()
            .map(|(key, _)| key.to_string())
            .filter(|key| !keep.contains(key.as_str()))
            .collect();
        for name in stale {
            providers.remove(&name);
        }
    }
    for p in &payload.providers {
        let name = p.name.trim().to_string();
        if name.is_empty() {
            continue;
        }
        let Some(provider) = providers
            .entry(&name)
            .or_insert(toml_edit::table())
            .as_table_mut()
        else {
            return Err(format!("配置节 `providers.{name}` 不是表,无法保存设置"));
        };
        settings_set_value(provider, "protocol", p.protocol.trim().to_string());
        settings_set_value(
            provider,
            "base_url",
            p.base_url.trim().trim_end_matches('/').to_string(),
        );
        settings_set_or_remove(
            provider,
            "api_key_env",
            p.api_key_env
                .as_ref()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
        );
        settings_set_or_remove(
            provider,
            "api_key",
            p.api_key
                .as_ref()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
        );
        settings_set_or_remove(
            provider,
            "auth",
            p.auth.as_ref().filter(|s| !s.is_empty()).cloned(),
        );
        match p.context_limit {
            Some(limit) => settings_set_value(provider, "context_limit", limit as i64),
            None => {
                provider.remove("context_limit");
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub fn settings_get(project_dir: Option<String>) -> serde_json::Value {
    let path = crate::global_config_path();
    let mut config: kanzei_harness::KanzeiConfig = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default();
    config.fill_defaults();
    let providers: Vec<serde_json::Value> = config
        .providers
        .iter()
        .map(|(name, p)| {
            let key_present = if p.api_key.as_deref().is_some_and(|k| !k.trim().is_empty()) {
                Some(true)
            } else {
                p.api_key_env
                    .as_deref()
                    .map(|env| std::env::var(env).map(|v| !v.is_empty()).unwrap_or(false))
            };
            json!({
                "name": name, "protocol": p.protocol, "baseUrl": p.base_url,
                "apiKeyEnv": p.api_key_env, "apiKey": p.api_key, "keyPresent": key_present,
                "auth": p.auth, "contextLimit": p.context_limit,
                // R-184 P6(D-246):内置 provider 由 fill_defaults 无条件回填,
                // 删了重开会回来——前端据此把删除入口换成「内置」标记,不误导。
                "builtin": kanzei_harness::config::builtin_provider_names().contains(&name.as_str()),
            })
        })
        .collect();
    // 生效值:全局 + 项目级合并后的结果。项目级 .kanzei/kanzei.toml 能覆盖的**每一项**
    // 都要报,漏报一项就意味着用户在本页改它、看到「已保存」、运行却永远用项目值
    // (D-168 当年只报了模型角色,[limits]/proxy/[profile] 这几项一直是哑的)。
    // 标量按与下面顶层字段**同一套**兜底归一化(proxy→env、profileDefault→dev、
    // codexFastMode→false),否则前端拿 None 和顶层的默认值一比就会天天误报覆盖。
    let effective = project_dir
        .as_deref()
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .and_then(|root| kanzei_harness::KanzeiConfig::load(&root).ok())
        .map(|merged| {
            json!({
                "primary": merged.models.primary, "fast": merged.models.fast,
                "reasoning": merged.models.reasoning,
                "codexFastMode": merged.models.codex_fast_mode.unwrap_or(false),
                "proxy": merged.proxy.clone().unwrap_or_else(|| "env".into()),
                "profileDefault": merged.profile.default.clone().unwrap_or_else(|| "dev".into()),
                "limits": {
                    "maxTokens": merged.limits.max_tokens,
                    "subagentMaxTokens": merged.limits.subagent_max_tokens,
                    "subagentTimeoutSecs": merged.limits.subagent_timeout_secs,
                    "contextBudgetRatio": merged.limits.context_budget_ratio,
                    "recentVerbatimRatio": merged.limits.recent_verbatim_ratio,
                    "maxTasksPerTurn": merged.limits.max_tasks_per_turn,
                    "maxParallelTools": merged.limits.max_parallel_tools,
                    "transportRetries": merged.limits.transport_retries,
                    "rateLimitRetries": merged.limits.rate_limit_retries,
                    "streamRestarts": merged.limits.stream_restarts,
                },
            })
        });
    json!({
        "path": path.display().to_string(), "primary": config.models.primary, "fast": config.models.fast,
        "proxy": config.proxy.unwrap_or_else(|| "env".into()),
        "profileDefault": config.profile.default.unwrap_or_else(|| "dev".into()),
        "reasoning": config.models.reasoning.unwrap_or_else(|| "off".into()),
        "codexFastMode": config.models.codex_fast_mode.unwrap_or(false), "providers": providers,
        // limits:发原始值(None = 表单留空),同时发一份生效默认值给占位符用——
        // 用户要看得见"留空等于多少",否则只能去翻源码。
        "limits": {
            "maxTokens": config.limits.max_tokens,
            "subagentMaxTokens": config.limits.subagent_max_tokens,
            "subagentTimeoutSecs": config.limits.subagent_timeout_secs,
            "contextBudgetRatio": config.limits.context_budget_ratio,
            "recentVerbatimRatio": config.limits.recent_verbatim_ratio,
            "maxTasksPerTurn": config.limits.max_tasks_per_turn,
            "maxParallelTools": config.limits.max_parallel_tools,
            "transportRetries": config.limits.transport_retries,
            "rateLimitRetries": config.limits.rate_limit_retries,
            "streamRestarts": config.limits.stream_restarts,
        },
        "limitDefaults": {
            "maxTokens": kanzei_harness::config::Limits::default().max_tokens(),
            "subagentMaxTokens": kanzei_harness::config::Limits::default().subagent_max_tokens(),
            "subagentTimeoutSecs": kanzei_harness::config::Limits::default().subagent_timeout_secs(),
            "contextBudgetRatio": kanzei_harness::config::Limits::default().context_budget_ratio(),
            "recentVerbatimRatio": kanzei_harness::config::Limits::default().recent_verbatim_ratio(),
            "maxTasksPerTurn": kanzei_harness::config::Limits::default().max_tasks_per_turn(),
            "maxParallelTools": kanzei_harness::config::Limits::default().max_parallel_tools(),
            "transportRetries": kanzei_harness::config::Limits::default().transport_retries(),
            "rateLimitRetries": kanzei_harness::config::Limits::default().rate_limit_retries(),
            "streamRestarts": kanzei_harness::config::Limits::default().stream_restarts(),
        },
        // 节奏(R-157):原值 + 生效默认值。前端把它渲染进继续文案规则 6,
        // 让"全量什么时候跑/定向跑不跑/提交与 push 频率"随配置变化而非硬化在提示词里。
        "cadence": serde_json::to_value(&config.cadence).unwrap_or_else(|_| serde_json::json!({})),
        "cadenceDefaults": serde_json::to_value(kanzei_harness::config::Cadence::default())
            .unwrap_or_else(|_| serde_json::json!({})),
        "effective": effective,
        "projectConfig": project_dir.as_deref().and_then(|d| kanzei_harness::config::discover_project_root(Path::new(d)))
            .map(|root| root.join(".kanzei").join("kanzei.toml").display().to_string()),
    })
}

pub(crate) fn settings_save_at_path_impl(
    payload: SettingsPayload,
    path: &Path,
) -> Result<(), String> {
    // 以现有配置文本为底,只改设置页管理的键:注释、排版、未知字段原样保留(D-082)。
    // 文件存在但解析失败必须报错——静默回退默认值再覆写等于销毁用户配置。
    let mut doc = settings_read_document(path)?;
    settings_apply_scalar_fields(&mut doc, &payload)?;
    settings_apply_limits(&mut doc, &payload)?;
    settings_apply_providers(&mut doc, &payload)?;
    settings_apply_cadence(&mut doc, &payload)?;
    settings_write_document(doc, path)
}

pub(crate) fn settings_save_at_path(payload: SettingsPayload, path: &Path) -> Result<(), String> {
    validate_model_roles(&payload)?;
    settings_save_at_path_impl(payload, path)
}

#[tauri::command]
pub fn settings_save(
    payload: SettingsPayload,
    scope: Option<String>,
    project_dir: Option<String>,
) -> Result<(), String> {
    validate_model_roles(&payload)?;
    // R-178 批4 D7:作用域选择器第一版只覆盖 [models]。选「本项目」时只把
    // 模型角色写进主根 .kanzei/kanzei.toml,proxy/profile/limits/cadence/providers
    // 一律不动——provider 密钥写进被 git 跟踪的项目 toml 有泄密风险(D7 边界)。
    // 选「全局」(默认,缺省即全局)行为与既有完全一致:全字段写全局配置。
    match scope.as_deref() {
        Some("project") => {
            let Some(dir) = project_dir.as_deref().filter(|d| !d.trim().is_empty()) else {
                return Err("「本项目」作用域需要当前项目目录".into());
            };
            let root = kanzei_harness::config::discover_project_root(Path::new(dir))
                .ok_or_else(|| format!("找不到项目根:{dir}"))?;
            let path = root.join(".kanzei").join("kanzei.toml");
            // 只应用模型字段;其余 apply_* 全部跳过。
            let mut doc = settings_read_document(&path)?;
            settings_apply_model_fields(&mut doc, &payload)?;
            settings_write_document(doc, &path)
        }
        _ => settings_save_at_path(payload, &global_config_path()),
    }
}
/// 「打开配置原文」在文件不存在时铺的底:**只有注释,一个键都不写**。
///
/// 此前这里是合成一份空 SettingsPayload 走 settings_save,结果 settings_apply_scalar_fields
/// 里 `codex_fast_mode` 是无条件写入,新文件一落地就是 `[models] codex_fast_mode = false`。
/// 而 `false` 与「未设」在配置里不可区分,config.rs 那条"primary 是 codex:gpt-5.6-luna
/// 且用户没表过态就自动开 Fast mode"从此永久失效——用户只是想看一眼配置长什么样,
/// 却把一个自己从没碰过的开关关死了。留空即默认,这是整套配置的基本约定。
pub(crate) fn settings_bootstrap_file(path: &Path) -> Result<(), String> {
    let template = "\
# kanzei 全局配置。留空 = 用内置默认;设置页里改的也是这个文件。\n\
# 常用节:[models] primary/fast/reasoning、proxy、[profile] default、\n\
#         [limits]、[cadence]、[providers.<名字>]\n";
    let doc: toml_edit::DocumentMut = template
        .parse()
        .map_err(|e| format!("内置配置模板不是合法 toml(这是 bug): {e}"))?;
    settings_write_document(doc, path)
}

#[tauri::command]
pub fn settings_open() -> Result<(), String> {
    let path = global_config_path();
    if !path.is_file() {
        settings_bootstrap_file(&path)?;
    }
    crate::state::hidden_command("cmd")
        .args(["/c", "start", "", &path.display().to_string()])
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}
fn project_permission_config(project_dir: &str) -> PathBuf {
    kanzei_harness::config::discover_project_root(Path::new(project_dir))
        .unwrap_or_else(|| PathBuf::from(project_dir))
        .join(".kanzei")
        .join("kanzei.toml")
}

#[tauri::command]
pub fn permission_rules_get(project_dir: String) -> Result<serde_json::Value, String> {
    let path = project_permission_config(&project_dir);
    let config: kanzei_harness::KanzeiConfig = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default();
    let rules = config.permissions.rules.iter().enumerate()
        .filter(|(_, rule)| rule.effect == kanzei_harness::permission::Effect::Allow)
        .map(|(index, rule)| json!({
            "index": index, "action": rule.action, "resource": rule.resource, "effect": rule.effect,
        }))
        .collect::<Vec<_>>();
    Ok(json!({ "path": path.display().to_string(), "rules": rules }))
}

#[tauri::command]
pub fn permission_rule_delete(project_dir: String, index: usize) -> Result<(), String> {
    let path = project_permission_config(&project_dir);
    let text =
        std::fs::read_to_string(&path).map_err(|error| format!("读取权限规则失败: {error}"))?;
    let mut config: kanzei_harness::KanzeiConfig =
        toml::from_str(&text).map_err(|error| format!("配置格式错误: {error}"))?;
    let Some(rule) = config.permissions.rules.get(index) else {
        return Err("权限规则不存在或已被删除".into());
    };
    if rule.effect != kanzei_harness::permission::Effect::Allow {
        return Err("只能删除已记住的放行规则".into());
    }
    config.permissions.rules.remove(index);
    let text = toml::to_string_pretty(&config).map_err(|error| error.to_string())?;
    std::fs::write(&path, text).map_err(|error| format!("写入权限规则失败: {error}"))
}
#[tauri::command]
pub async fn provider_test(
    protocol: String,
    base_url: String,
    api_key_env: Option<String>,
    api_key: Option<String>,
    auth: Option<String>,
    proxy: Option<String>,
) -> Result<String, String> {
    if matches!(auth.as_deref(), Some("codex") | Some("claude")) {
        return Ok("订阅登录态通道,无需 key 测试".into());
    }
    let key = api_key
        .filter(|k| !k.trim().is_empty())
        .or_else(|| api_key_env.as_deref().and_then(|e| std::env::var(e).ok()))
        .filter(|k| !k.trim().is_empty());
    let config = kanzei_harness::KanzeiConfig::load(Path::new(".")).unwrap_or_default();
    let proxy_value = proxy.or(config.proxy);
    let proxy = match proxy_value.as_deref() {
        Some("off") => kanzei_llm::ProxyConfig::Disabled,
        Some("env") | None => kanzei_llm::ProxyConfig::Env,
        Some(p) => kanzei_llm::ProxyConfig::Explicit(p.to_string()),
    };
    let client = kanzei_llm::proxy::build_http_client(&proxy).map_err(|e| e.to_string())?;
    let base = base_url.trim_end_matches('/');
    let request = match protocol.as_str() {
        "anthropic" => {
            let mut request = client
                .get(format!("{base}/v1/models"))
                .header("anthropic-version", "2023-06-01");
            if let Some(key) = &key {
                request = request.header("x-api-key", key);
            }
            request
        }
        _ => {
            let mut request = client.get(format!("{base}/models"));
            if let Some(key) = &key {
                request = request.bearer_auth(key);
            }
            request
        }
    };
    match request
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
    {
        Ok(response) => {
            let status = response.status().as_u16();
            Ok(match status {
                200 => format!("✓ 可用(HTTP 200{})", if key.is_some() { ",key 有效" } else { ",无鉴权" }),
                401 | 403 => format!("✗ key 无效(HTTP {status})——检查 key 是否过期/复制完整;moonshot 注意 .cn 与 .ai 的 key 不通用"),
                404 => "✗ 端点 404——base_url 可能不对(需要以 /v1 结尾?)".into(),
                _ => format!("? HTTP {status}——通道可达但响应异常"),
            })
        }
        Err(error) if error.is_timeout() => {
            Ok("✗ 超时——检查网络/代理设置(本地服务不走代理)".into())
        }
        Err(error) if error.is_connect() => Ok("✗ 连接失败——服务未启动或代理不通".into()),
        Err(error) => Ok(format!("✗ 请求失败:{error}")),
    }
}

#[allow(dead_code)]
fn _state_type_marker(_: Option<State<'_, AppState>>) {}

#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_harness::KanzeiConfig;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn 运行上限只写填了的键_留空的键从配置里移除() {
        let path = std::env::temp_dir().join(format!(
            "kanzei-limits-{}.toml",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let payload = |limits: LimitsPayload| SettingsPayload {
            primary: String::new(),
            fast: String::new(),
            proxy: "env".into(),
            reasoning: None,
            codex_fast_mode: false,
            profile_default: None,
            profile: None,
            limits,
            providers: vec![],
            cadence: None,
        };
        settings_save_at_path(
            payload(LimitsPayload {
                max_tokens: Some(16384),
                subagent_timeout_secs: Some(300),
                ..Default::default()
            }),
            &path,
        )
        .unwrap();
        let saved: KanzeiConfig = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved.limits.max_tokens, Some(16384));
        assert_eq!(saved.limits.subagent_timeout_secs, Some(300));
        assert_eq!(
            saved.limits.max_tasks_per_turn, None,
            "没填的键不该被写进文件"
        );
        assert_eq!(saved.limits.max_tasks_per_turn(), 16, "没填就走内置默认");
        settings_save_at_path(payload(LimitsPayload::default()), &path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("max_tokens"), "清空后仍留着死键:\n{text}");
        let saved: KanzeiConfig = toml::from_str(&text).unwrap();
        assert_eq!(saved.limits.max_tokens(), 8192);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn settings_save_preserves_handwritten_permission_rules() {
        let path = std::env::temp_dir().join(format!(
            "kanzei-settings-{}.toml",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &path,
            "[permissions]\n[[permissions.rules]]\naction = \"bash\"\nresource = \"{\\\"command\\\":\\\"git status\\\",\\\"workdir\\\":\\\".\\\"}\"\neffect = \"allow\"\n",
        ).unwrap();
        settings_save_at_path(
            SettingsPayload {
                primary: "anthropic:claude-sonnet-5".into(),
                fast: String::new(),
                proxy: "env".into(),
                reasoning: None,
                codex_fast_mode: false,
                profile_default: None,
                profile: None,
                limits: Default::default(),
                providers: vec![],
                cadence: None,
            },
            &path,
        )
        .unwrap();
        let config: KanzeiConfig =
            toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(config.permissions.rules.len(), 1);
        assert_eq!(config.permissions.rules[0].action, "bash");
        assert_eq!(
            config.models.primary.as_deref(),
            Some("anthropic:claude-sonnet-5")
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn settings_save_preserves_comments_and_unknown_fields() {
        let path = std::env::temp_dir().join(format!(
            "kanzei-settings-preserve-{}.toml",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &path,
            "# 顶部注释:手写配置\nproxy = \"http://127.0.0.1:7890\"\n\n[models]\nprimary = \"anthropic:claude-sonnet-5\" # 主模型\n\n[future_section]\nnew_field = \"来自新版本\"\n\n[[permissions.rules]]\naction = \"read\"\nresource = \"*/.env\"\neffect = \"deny\"\n",
        ).unwrap();
        settings_save_at_path(
            SettingsPayload {
                primary: "anthropic:claude-opus-5".into(),
                fast: "ollama:qwen3.5:4b".into(),
                proxy: "env".into(),
                reasoning: Some("high".into()),
                codex_fast_mode: true,
                profile_default: Some("dev".into()),
                profile: None,
                limits: Default::default(),
                providers: vec![],
                cadence: None,
            },
            &path,
        )
        .unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        for expected in [
            "# 顶部注释:手写配置",
            "# 主模型",
            "[future_section]",
            "new_field = \"来自新版本\"",
        ] {
            assert!(
                saved.contains(expected),
                "missing preserved text: {expected}\n---\n{saved}"
            );
        }
        assert!(
            saved.contains("proxy = \"env\""),
            "proxy should reset to env:\n{saved}"
        );
        let config: KanzeiConfig = toml::from_str(&saved).unwrap();
        assert_eq!(
            config.models.primary.as_deref(),
            Some("anthropic:claude-opus-5")
        );
        assert_eq!(config.models.reasoning.as_deref(), Some("high"));
        assert_eq!(config.models.codex_fast_mode, Some(true));
        assert_eq!(config.permissions.rules.len(), 1);
        assert_eq!(config.permissions.rules[0].resource, "*/.env");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn settings_save_refuses_to_overwrite_unparseable_config() {
        let path = std::env::temp_dir().join(format!(
            "kanzei-settings-broken-{}.toml",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let broken = "[models\nprimary = 不是合法 toml";
        std::fs::write(&path, broken).unwrap();
        let result = settings_save_at_path(
            SettingsPayload {
                primary: "anthropic:claude-sonnet-5".into(),
                fast: String::new(),
                proxy: "env".into(),
                reasoning: None,
                codex_fast_mode: false,
                profile_default: None,
                profile: None,
                limits: Default::default(),
                providers: vec![],
                cadence: None,
            },
            &path,
        );
        assert!(result.is_err(), "saving over a broken config must fail");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            broken,
            "file must be untouched"
        );
        let _ = std::fs::remove_file(path);
    }

    fn 空载荷(providers: Vec<ProviderPayload>) -> SettingsPayload {
        SettingsPayload {
            primary: String::new(),
            fast: String::new(),
            proxy: "env".into(),
            reasoning: None,
            codex_fast_mode: false,
            profile_default: None,
            profile: None,
            limits: Default::default(),
            providers,
            cadence: None,
        }
    }

    fn 临时配置(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "kanzei-{tag}-{}.toml",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn provider_清单非空即权威_删掉的节要真消失_空清单则整节不动() {
        let path = 临时配置("providers-prune");
        std::fs::write(
            &path,
            "[providers.mine]\nprotocol = \"openai\"\nbase_url = \"http://mine\"\n\
             [providers.keepme]\nprotocol = \"openai\"\nbase_url = \"http://keep\"\n",
        )
        .unwrap();
        // 设置页删掉 mine 后发的就是这张"少了一行"的整表。
        settings_save_at_path(
            空载荷(vec![ProviderPayload {
                name: "keepme".into(),
                protocol: "openai".into(),
                base_url: "http://keep".into(),
                api_key_env: None,
                api_key: None,
                auth: None,
                context_limit: None,
            }]),
            &path,
        )
        .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            !text.contains("[providers.mine]"),
            "表格里删掉的 provider 仍留在配置里(重开设置页会原样回来):\n{text}"
        );
        let saved: KanzeiConfig = toml::from_str(&text).unwrap();
        assert!(
            saved.providers.contains_key("keepme"),
            "清单里的 provider 被误删"
        );
        assert!(!saved.providers.contains_key("mine"));

        // 载荷不带 provider(settings_open 合成载荷 / 其他调用方)→ 只增改不删。
        settings_save_at_path(空载荷(vec![]), &path).unwrap();
        let saved: KanzeiConfig = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(
            saved.providers.contains_key("keepme"),
            "空清单不该被当成「删光所有 provider」"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn 打开配置原文_新建文件不把_codex_fast_mode_写死成_false() {
        let path = 临时配置("bootstrap");
        settings_bootstrap_file(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            !text.contains("codex_fast_mode"),
            "新建的配置写死了开关,「未设」与 false 从此不可区分:\n{text}"
        );
        let mut config: KanzeiConfig = toml::from_str(&text).unwrap();
        assert_eq!(
            config.models.codex_fast_mode, None,
            "新文件必须是「未表态」"
        );
        config.fill_defaults();
        assert_eq!(
            config.models.codex_fast_mode,
            Some(true),
            "留空才能让 codex:gpt-5.6-luna 的 Fast mode 自动开启继续生效"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn readonly_档位保存后不被静默降级成_dev() {
        let path = 临时配置("profile-readonly");
        std::fs::write(&path, "[profile]\ndefault = \"readonly\"\n").unwrap();
        let mut payload = 空载荷(vec![]);
        payload.profile_default = Some("readonly".into());
        settings_save_at_path(payload, &path).unwrap();
        let saved: KanzeiConfig = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            saved.profile.default.as_deref(),
            Some("readonly"),
            "路过一次设置页就把 readonly 降级成 dev"
        );
        // 非法值仍然要被挡下并回落 dev(白名单放宽不等于不校验)。
        let mut payload = 空载荷(vec![]);
        payload.profile_default = Some("不存在的档".into());
        settings_save_at_path(payload, &path).unwrap();
        let saved: KanzeiConfig = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved.profile.default.as_deref(), Some("dev"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn 节奏字段_写入读回_清空移除_不串改其他键() {
        let path = std::env::temp_dir().join(format!(
            "kanzei-cadence-{}.toml",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let payload = |cadence: Option<CadencePayload>| SettingsPayload {
            primary: String::new(),
            fast: String::new(),
            proxy: "env".into(),
            reasoning: None,
            codex_fast_mode: false,
            profile_default: None,
            profile: None,
            limits: Default::default(),
            providers: vec![],
            cadence,
        };
        // 写满全部字段 → 读回一致。
        settings_save_at_path(
            payload(Some(CadencePayload {
                full_test: Some("every_n_batches".into()),
                full_test_batches: Some(3),
                targeted_test: Some("every_commit".into()),
                commit: Some("per_entry".into()),
                push: Some("periodic".into()),
            })),
            &path,
        )
        .unwrap();
        let saved: KanzeiConfig = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            saved.cadence.full_test,
            kanzei_harness::config::FullTestCadence::EveryNBatches
        );
        assert_eq!(saved.cadence.full_test_batches, Some(3));
        assert_eq!(
            saved.cadence.targeted_test,
            kanzei_harness::config::TargetedTestCadence::EveryCommit
        );
        assert_eq!(
            saved.cadence.commit,
            kanzei_harness::config::CommitCadence::PerEntry
        );
        assert_eq!(
            saved.cadence.push,
            kanzei_harness::config::PushCadence::Periodic
        );

        // 清空(cadence 节带全空字段)→ 键从文件移除,回落 §1.4 默认。
        settings_save_at_path(
            payload(Some(CadencePayload {
                full_test: None,
                full_test_batches: None,
                targeted_test: None,
                commit: None,
                push: None,
            })),
            &path,
        )
        .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            !text.contains("[cadence]"),
            "清空后仍留 [cadence] 节:\n{text}"
        );
        let saved: KanzeiConfig = toml::from_str(&text).unwrap();
        assert_eq!(
            saved.cadence.full_test,
            kanzei_harness::config::FullTestCadence::EntryClose
        );
        assert_eq!(
            saved.cadence.push,
            kanzei_harness::config::PushCadence::PerEntry
        );

        // 载荷不带 cadence(旧前端/其他调用方)→ 既有节原样保留,不删不改。
        std::fs::write(&path, "[cadence]\nfull_test = \"release_only\"\n").unwrap();
        settings_save_at_path(payload(None), &path).unwrap();
        let saved: KanzeiConfig = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            saved.cadence.full_test,
            kanzei_harness::config::FullTestCadence::ReleaseOnly,
            "载荷缺 cadence 时不得动既有节"
        );
        let _ = std::fs::remove_file(path);
    }

    /// R-178 批4 D7:作用域选择器第一版只覆盖 [models]。
    /// - scope=project → 只把模型角色写进主根 .kanzei/kanzei.toml,proxy/provider 不串写;
    /// - scope=global(缺省)→ 与既有 settings_save 行为一致,项目配置不被触碰。
    #[test]
    fn settings_save_project_scope_writes_only_models() {
        let project_root = std::env::temp_dir().join(format!(
            "kanzei-d7-root-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let kanzei_dir = project_root.join(".kanzei");
        std::fs::create_dir_all(&kanzei_dir).unwrap();
        let project_toml = kanzei_dir.join("kanzei.toml");
        std::fs::write(
            &project_toml,
            "# 项目配置\nproxy = \"off\"\n[providers.stub]\nprotocol = \"openai\"\nbase_url = \"http://x\"\n",
        )
        .unwrap();

        // 载荷里 proxy 与 providers 都非空——scope=project 必须把它们挡在项目配置外。
        let mut payload = 空载荷(vec![ProviderPayload {
            name: "stub".into(),
            protocol: "openai".into(),
            base_url: "http://x".into(),
            api_key_env: Some("STUB_KEY".into()),
            api_key: None,
            auth: None,
            context_limit: None,
        }]);
        payload.proxy = "http://127.0.0.1:12000".into();
        payload.primary = "codex:gpt-5.6-luna".into();
        payload.fast = "codex:gpt-5.6-luna-fast".into();

        settings_save(
            payload,
            Some("project".into()),
            Some(project_root.display().to_string()),
        )
        .unwrap();

        let text = std::fs::read_to_string(&project_toml).unwrap();
        assert!(
            text.contains("primary = \"codex:gpt-5.6-luna\""),
            "模型角色未写入项目配置:\n{text}"
        );
        assert!(
            text.contains("fast = \"codex:gpt-5.6-luna-fast\""),
            "fast 未写入:\n{text}"
        );
        assert!(
            !text.contains("127.0.0.1:12000"),
            "proxy 被串写进项目配置:\n{text}"
        );
        assert!(
            !text.contains("STUB_KEY"),
            "provider 密钥被串写进项目配置:\n{text}"
        );
        assert!(
            text.contains("[providers.stub]"),
            "既有 providers 节被误删:\n{text}"
        );
        // 注释与未知内容保留(toml_edit 文档化编辑)。
        assert!(text.contains("# 项目配置"), "注释丢失:\n{text}");
        let _ = std::fs::remove_dir_all(&project_root);
    }

    /// D7 收尾:scope=project 且缺项目目录 → 报错而不是静默写全局(避免把「本项目」
    /// 的意图落进全局配置——那正是 D-248 那类静默降级的复发形态)。
    #[test]
    fn settings_save_project_scope_without_project_dir_refuses() {
        let result = settings_save(空载荷(vec![]), Some("project".into()), None);
        assert!(result.is_err(), "缺项目目录必须报错,不得静默落全局");
    }
}
