//! UI-0926 #3:模型配置的**展示真源**与项目级模型覆盖的逐键编辑。
//!
//! 为什么单独一个文件:模型配置有五层(本线临时覆盖 → agent 定义 → 项目 `[models]` →
//! 全局 `[models]` → 内置默认),此前设置页、输入框上方的下拉、下拉里的角色项各看一层,
//! 三处说三个模型。这里把「下一轮到底用哪个模型/哪一档思考/Fast mode 开没开、各来自
//! 哪一层」算成**一个对象**,输入框芯片、菜单高亮、tooltip 都只从它渲染。
//!
//! 同源纪律:解析一律调运行路径自己的函数——`resolve_model_chain`(引用层合并)、
//! `KanzeiConfig::resolve_model`(角色 → provider:model)、`resolve_reasoning_override`
//! (思考档)、`KanzeiConfig::service_tier_for`(Fast mode 是否真生效)。本文件只额外做
//! 「来源归属」这一件事(读两层原文判断值来自哪一层),不自己重算任何生效值。
//! `turn_view_matches_runner_path` 测试守着这条纪律。
//!
//! 三个编辑面各写一层、互不越界:设置页只写全局;本文件的 `project_models_save` 只写项目
//! 文件的 `[models]` 五个键(逐键覆盖或恢复继承);输入框芯片只写本线(process_update)。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::State;

use kanzei_harness::config::{KanzeiConfig, ModelRoles};

use crate::{AppState, MutexPoisonExt};

/// 值来自哪一层。前端按它显示「临时 / 项目 / 全局 / 内置 / Agent」。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Source {
    Line,
    Agent,
    Project,
    Global,
    Builtin,
}

/// 两层配置文件里 `[models]` 的**原样**内容(不跑 fill_defaults),只用来判断来源。
#[derive(Debug, Clone, Default)]
pub(crate) struct ModelLayers {
    pub(crate) global: ModelRoles,
    pub(crate) project: ModelRoles,
}

/// 项目模型配置能编辑的五个键(前端 camelCase ↔ 配置 snake_case)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ModelKey {
    Primary,
    Fast,
    Compact,
    Reasoning,
    CodexFastMode,
}

impl ModelKey {
    pub(crate) const ALL: [ModelKey; 5] = [
        ModelKey::Primary,
        ModelKey::Fast,
        ModelKey::Compact,
        ModelKey::Reasoning,
        ModelKey::CodexFastMode,
    ];

    pub(crate) fn camel(self) -> &'static str {
        match self {
            ModelKey::Primary => "primary",
            ModelKey::Fast => "fast",
            ModelKey::Compact => "compact",
            ModelKey::Reasoning => "reasoning",
            ModelKey::CodexFastMode => "codexFastMode",
        }
    }

    fn snake(self) -> &'static str {
        match self {
            ModelKey::CodexFastMode => "codex_fast_mode",
            other => other.camel(),
        }
    }

    pub(crate) fn from_camel(key: &str) -> Option<ModelKey> {
        ModelKey::ALL.into_iter().find(|k| k.camel() == key)
    }
}

/// 设置 reasoning 时接受的档位(与 ReasoningEffort 的取值一一对应)。
const REASONING_LEVELS: [&str; 7] = ["off", "none", "low", "medium", "high", "xhigh", "max"];

fn nonblank(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|v| !v.is_empty())
}

fn role_of(reference: &str) -> Option<&'static str> {
    match reference.trim() {
        "primary" => Some("primary"),
        "fast" => Some("fast"),
        "compact" => Some("compact"),
        _ => None,
    }
}

/// 内置 primary:不抄字面量,直接问 fill_defaults(与运行路径同一个兜底)。
fn builtin_primary() -> String {
    let mut probe = KanzeiConfig::default();
    probe.fill_defaults();
    probe.models.primary.unwrap_or_default()
}

/// 两层都没表态时 Fast mode 的内置结论:同样交给 fill_defaults 判(primary 是默认 Luna 才自动开)。
fn builtin_fast_mode(effective_primary: Option<&str>) -> bool {
    let mut probe = KanzeiConfig::default();
    probe.models.primary = effective_primary.map(str::to_string);
    probe.fill_defaults();
    probe.models.codex_fast_mode.unwrap_or(false)
}

// ---------- 读两层原文 ----------

/// 原样解析一个配置文件的 `[models]`;文件不存在 = 全空,解析失败带路径报错。
pub(crate) fn read_models_raw(path: &Path) -> Result<ModelRoles, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str::<KanzeiConfig>(&text)
            .map(|config| config.models)
            .map_err(|e| format!("配置无法解析 {}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ModelRoles::default()),
        Err(e) => Err(format!("读取配置失败 {}: {e}", path.display())),
    }
}

fn project_config_path(root: &Path) -> PathBuf {
    root.join(".kanzei").join("kanzei.toml")
}

pub(crate) fn read_model_layers_at(
    global_path: &Path,
    project_path: Option<&Path>,
) -> Result<ModelLayers, String> {
    Ok(ModelLayers {
        global: read_models_raw(global_path)?,
        project: match project_path {
            Some(path) => read_models_raw(path)?,
            None => ModelRoles::default(),
        },
    })
}

/// 全局 `kanzei_home()/kanzei.toml` 与 `root/.kanzei/kanzei.toml` 的 `[models]` 原文。
pub(crate) fn read_model_layers(root: Option<&Path>) -> Result<ModelLayers, String> {
    let project = root.map(project_config_path);
    read_model_layers_at(&crate::global_config_path(), project.as_deref())
}

/// 与 run_prompt 同一个取根口径:发现式主根,找不到就用目录本身。
pub(crate) fn main_root(project_dir: &str) -> PathBuf {
    kanzei_harness::config::discover_project_root(Path::new(project_dir))
        .unwrap_or_else(|| PathBuf::from(project_dir))
}

/// 合并配置:有项目走运行路径同一个 `load_at_root`;没有项目只看全局 + 内置。
fn load_merged(root: Option<&Path>) -> Result<KanzeiConfig, String> {
    match root {
        Some(root) => KanzeiConfig::load_at_root(root).map_err(|e| format!("读取配置失败: {e}")),
        None => {
            let path = crate::global_config_path();
            let mut config = match std::fs::read_to_string(&path) {
                Ok(text) => toml::from_str::<KanzeiConfig>(&text)
                    .map_err(|e| format!("配置无法解析 {}: {e}", path.display()))?,
                Err(_) => KanzeiConfig::default(),
            };
            config.fill_defaults();
            Ok(config)
        }
    }
}

// ---------- 单字段视图(项目模型配置弹窗一行) ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FieldView {
    /// 项目文件里写的值(null = 没写,即继承)。
    pub(crate) project: Value,
    /// 全局文件里写的值(null = 没写)。
    pub(crate) global: Value,
    /// 若项目不写这一键,生效值是什么(继承全局 / 内置 / 跟随 primary)。
    pub(crate) inherited: Value,
    pub(crate) inherited_source: Source,
    pub(crate) inherited_follows_primary: bool,
    /// 合并后的生效值。
    pub(crate) effective: Value,
    pub(crate) source: Source,
    pub(crate) follows_primary: bool,
}

fn primary_source(layers: &ModelLayers) -> Source {
    if nonblank(&layers.project.primary).is_some() {
        Source::Project
    } else if nonblank(&layers.global.primary).is_some() {
        Source::Global
    } else {
        Source::Builtin
    }
}

fn string_value(value: Option<&str>) -> Value {
    value.map_or(Value::Null, |v| Value::String(v.to_string()))
}

/// 一个键的「项目 / 全局 / 继承 / 生效 / 来源」。merged 是运行路径的合并配置(已 fill_defaults)。
pub(crate) fn field_view(layers: &ModelLayers, merged: &KanzeiConfig, key: ModelKey) -> FieldView {
    let merged_primary = merged.models.primary.as_deref();
    match key {
        ModelKey::Primary => {
            let project = nonblank(&layers.project.primary);
            let global = nonblank(&layers.global.primary);
            let builtin = builtin_primary();
            let (inherited, inherited_source) = match global {
                Some(g) => (g.to_string(), Source::Global),
                None => (builtin, Source::Builtin),
            };
            FieldView {
                project: string_value(project),
                global: string_value(global),
                inherited: Value::String(inherited),
                inherited_source,
                inherited_follows_primary: false,
                effective: string_value(merged_primary),
                source: primary_source(layers),
                follows_primary: false,
            }
        }
        ModelKey::Fast | ModelKey::Compact => {
            fn pick(roles: &ModelRoles, key: ModelKey) -> Option<&str> {
                if key == ModelKey::Fast {
                    nonblank(&roles.fast)
                } else {
                    nonblank(&roles.compact)
                }
            }
            let project = pick(&layers.project, key);
            let global = pick(&layers.global, key);
            // 项目不写这一键时:全局写了用全局,都没写就跟随(合并后的)有效 primary。
            let (inherited, inherited_source, inherited_follows) = match global {
                Some(g) => (Some(g), Source::Global, false),
                None => (merged_primary, primary_source(layers), true),
            };
            let (effective, source, follows) = match (project, global) {
                (Some(p), _) => (Some(p), Source::Project, false),
                (None, Some(g)) => (Some(g), Source::Global, false),
                (None, None) => (merged_primary, primary_source(layers), true),
            };
            FieldView {
                project: string_value(project),
                global: string_value(global),
                inherited: string_value(inherited),
                inherited_source,
                inherited_follows_primary: inherited_follows,
                effective: string_value(effective),
                source,
                follows_primary: follows,
            }
        }
        ModelKey::Reasoning => {
            let project = nonblank(&layers.project.reasoning);
            let global = nonblank(&layers.global.reasoning);
            let builtin = kanzei_llm::ReasoningEffort::default().as_str();
            let (inherited, inherited_source) = match global {
                Some(g) => (g, Source::Global),
                None => (builtin, Source::Builtin),
            };
            let source = if project.is_some() {
                Source::Project
            } else {
                inherited_source
            };
            FieldView {
                project: string_value(project),
                global: string_value(global),
                inherited: Value::String(inherited.to_string()),
                inherited_source,
                inherited_follows_primary: false,
                // 生效档用运行路径的判据(未知值回落 off),与芯片、请求一致。
                effective: Value::String(
                    kanzei_tools::run::resolve_reasoning_override(
                        None,
                        merged.models.reasoning.as_deref(),
                    )
                    .as_str()
                    .to_string(),
                ),
                source,
                follows_primary: false,
            }
        }
        ModelKey::CodexFastMode => {
            let project = layers.project.codex_fast_mode;
            let global = layers.global.codex_fast_mode;
            let (inherited, inherited_source) = match global {
                Some(g) => (g, Source::Global),
                None => (builtin_fast_mode(merged_primary), Source::Builtin),
            };
            let source = if project.is_some() {
                Source::Project
            } else {
                inherited_source
            };
            FieldView {
                project: project.map_or(Value::Null, Value::Bool),
                global: global.map_or(Value::Null, Value::Bool),
                inherited: Value::Bool(inherited),
                inherited_source,
                inherited_follows_primary: false,
                effective: Value::Bool(merged.models.codex_fast_mode.unwrap_or(false)),
                source,
                follows_primary: false,
            }
        }
    }
}

// ---------- 下一轮视图(输入框芯片) ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelChoice {
    /// 交给 resolve_model 的引用(角色名或 provider:model)。
    #[serde(rename = "ref")]
    pub(crate) reference: String,
    /// 解析出的 provider:model;解析失败为 null(见 error)。
    pub(crate) resolved: Option<String>,
    pub(crate) source: Source,
    /// 引用是角色时的角色名(primary/fast/compact)。
    pub(crate) role: Option<String>,
    /// fast/compact 两层都没写、实际跟随 primary。
    pub(crate) follows_primary: bool,
    pub(crate) error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReasoningChoice {
    pub(crate) value: String,
    pub(crate) source: Source,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FastModeView {
    /// 配置开关(合并后)。
    pub(crate) enabled: bool,
    /// 下一轮解析到的 provider 是不是 Codex 订阅通道(不是则开关不起作用)。
    pub(crate) applies: bool,
    /// 真正生效 = service_tier_for 给出 priority(运行路径同一判据)。
    pub(crate) active: bool,
    pub(crate) source: Source,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TurnView {
    pub(crate) agent: Option<String>,
    pub(crate) agent_error: Option<String>,
    /// 下一轮真正会用的模型(含本线临时覆盖)。
    pub(crate) model: ModelChoice,
    /// 不带本线覆盖时的默认(「跟随默认」那一项显示它)。
    pub(crate) default_model: ModelChoice,
    pub(crate) reasoning: ReasoningChoice,
    pub(crate) default_reasoning: ReasoningChoice,
    pub(crate) codex_fast_mode: FastModeView,
    pub(crate) context_limit: Option<u64>,
}

fn model_choice(
    merged: &KanzeiConfig,
    layers: &ModelLayers,
    agent_model: &str,
    line_model: Option<&str>,
) -> (ModelChoice, Option<kanzei_harness::config::ResolvedModel>) {
    let line_model = line_model.map(str::trim).filter(|v| !v.is_empty());
    // 运行路径:run_prompt 把本线模型作为引用层传给 resolve_model_chain,再 resolve_model。
    let reference = kanzei_harness::config::resolve_model_chain(None, line_model, agent_model);
    let role = role_of(&reference);
    let (source, follows_primary) = if line_model.is_some() {
        (Source::Line, false)
    } else if let Some(role) = role {
        let key = match role {
            "fast" => ModelKey::Fast,
            "compact" => ModelKey::Compact,
            _ => ModelKey::Primary,
        };
        let view = field_view(layers, merged, key);
        (view.source, view.follows_primary)
    } else {
        (Source::Agent, false)
    };
    let resolved = merged.resolve_model(&reference);
    let choice = ModelChoice {
        reference,
        resolved: resolved
            .as_ref()
            .ok()
            .map(|r| format!("{}:{}", r.provider_name, r.model)),
        source,
        role: role.map(str::to_string),
        follows_primary,
        error: resolved.as_ref().err().map(|e| e.to_string()),
    };
    (choice, resolved.ok())
}

fn reasoning_source(layers: &ModelLayers) -> Source {
    if nonblank(&layers.project.reasoning).is_some() {
        Source::Project
    } else if nonblank(&layers.global.reasoning).is_some() {
        Source::Global
    } else {
        Source::Builtin
    }
}

/// 下一轮的完整视图。`agent_model` 是 agent 定义里的模型引用,`line_*` 是本线存档
/// (state.db processes 表,process_update 写入;空串已在写入时归一成 None)。
pub(crate) fn turn_view(
    merged: &KanzeiConfig,
    layers: &ModelLayers,
    agent_model: &str,
    line_model: Option<&str>,
    line_reasoning: Option<&str>,
) -> TurnView {
    let (model, resolved) = model_choice(merged, layers, agent_model, line_model);
    let (default_model, _) = model_choice(merged, layers, agent_model, None);
    let line_reasoning = line_reasoning.map(str::trim).filter(|v| !v.is_empty());
    let configured = merged.models.reasoning.as_deref();
    let reasoning = ReasoningChoice {
        value: kanzei_tools::run::resolve_reasoning_override(line_reasoning, configured)
            .as_str()
            .to_string(),
        source: if line_reasoning.is_some() {
            Source::Line
        } else {
            reasoning_source(layers)
        },
    };
    let default_reasoning = ReasoningChoice {
        value: kanzei_tools::run::resolve_reasoning_override(None, configured)
            .as_str()
            .to_string(),
        source: reasoning_source(layers),
    };
    let fast = field_view(layers, merged, ModelKey::CodexFastMode);
    let codex_fast_mode = FastModeView {
        enabled: merged.models.codex_fast_mode.unwrap_or(false),
        applies: resolved
            .as_ref()
            .is_some_and(|r| r.provider.auth.as_deref() == Some("codex")),
        active: resolved
            .as_ref()
            .is_some_and(|r| merged.service_tier_for(r).is_some()),
        source: fast.source,
    };
    TurnView {
        agent: None,
        agent_error: None,
        context_limit: resolved.as_ref().and_then(|r| r.provider.context_limit),
        model,
        default_model,
        reasoning,
        default_reasoning,
        codex_fast_mode,
    }
}

/// 桌面端按 agent 名取 agent 定义里的模型引用——与 run_task 同一套 harness 装配
/// (agent_directory_get 也是这么取的)。失败回落 "primary" 并把原因交给前端显示。
fn agent_model_for(
    root: Option<&Path>,
    merged: &KanzeiConfig,
    profile: Option<&str>,
    agent: Option<&str>,
) -> (Option<String>, String, Option<String>) {
    let resolve = || -> anyhow::Result<(String, String)> {
        let profile = crate::run::assembly::resolve_profile(profile, merged)?;
        let cwd = root
            .map(Path::to_path_buf)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        let ctx = kanzei_harness::ResolveCtx {
            profile,
            cwd: cwd.clone(),
            project_root: cwd,
            config: std::sync::Arc::new(merged.clone()),
        };
        let snapshot = crate::run::assembly::build_run_harness(false, None).resolve(&ctx)?;
        let agent = snapshot.select_agent(agent.filter(|a| !a.trim().is_empty()))?;
        Ok((agent.name.clone(), agent.model.clone()))
    };
    match resolve() {
        Ok((name, model)) => (Some(name), model, None),
        Err(error) => (
            agent.map(str::to_string),
            "primary".into(),
            Some(error.to_string()),
        ),
    }
}

/// 输入框芯片的唯一数据源:下一轮会用哪个模型/哪一档思考/Fast mode,各来自哪一层。
///
/// 同步命令(只读配置与 agent 定义,不探测网络)。解析失败不抛错:model.resolved=null、
/// model.error 带原因;只有配置文件本身读不了才返回 Err。
#[tauri::command]
pub(crate) fn model_effective(
    state: State<'_, AppState>,
    project_dir: Option<String>,
    process_id: Option<String>,
    profile: Option<String>,
    agent: Option<String>,
) -> Result<Value, String> {
    let root = project_dir
        .as_deref()
        .map(str::trim)
        .filter(|dir| !dir.is_empty())
        .map(main_root);
    let merged = load_merged(root.as_deref())?;
    let layers = read_model_layers(root.as_deref())?;
    let (agent_name, agent_model, agent_error) = agent_model_for(
        root.as_deref(),
        &merged,
        profile.as_deref(),
        agent.as_deref(),
    );
    let (line_model, line_reasoning) = process_id
        .as_deref()
        .and_then(|id| state.processes.lock_or_recover().get(id).cloned())
        .map(|process| {
            (
                process.model.lock_or_recover().clone(),
                process.reasoning.lock_or_recover().clone(),
            )
        })
        .unwrap_or((None, None));
    let mut view = turn_view(
        &merged,
        &layers,
        &agent_model,
        line_model.as_deref(),
        line_reasoning.as_deref(),
    );
    view.agent = agent_name;
    view.agent_error = agent_error;
    serde_json::to_value(view).map_err(|e| e.to_string())
}

// ---------- 项目模型配置(弹窗) ----------

pub(crate) fn project_models_view(
    project_dir: &str,
    root: &Path,
    layers: &ModelLayers,
    merged: &KanzeiConfig,
) -> Value {
    let path = project_config_path(root);
    let mut fields = serde_json::Map::new();
    for key in ModelKey::ALL {
        fields.insert(
            key.camel().to_string(),
            serde_json::to_value(field_view(layers, merged, key)).unwrap_or(Value::Null),
        );
    }
    json!({
        "projectDir": project_dir,
        "configPath": path.display().to_string(),
        "exists": path.is_file(),
        "fields": fields,
    })
}

fn project_models_get_at(project_dir: &str) -> Result<Value, String> {
    let root = main_root(project_dir);
    let layers = read_model_layers(Some(&root))?;
    let merged = load_merged(Some(&root))?;
    Ok(project_models_view(project_dir, &root, &layers, &merged))
}

#[tauri::command]
pub(crate) fn project_models_get(project_dir: String) -> Result<Value, String> {
    project_models_get_at(&project_dir)
}

/// 要写进项目 `[models]` 的键。未知键(例如 proxy、providers)在反序列化时就被拒——
/// 这个入口只管模型,绝不把别的东西(尤其 provider 密钥)写进被 git 跟踪的项目文件。
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProjectModelsSet {
    #[serde(default)]
    pub(crate) primary: Option<String>,
    #[serde(default)]
    pub(crate) fast: Option<String>,
    #[serde(default)]
    pub(crate) compact: Option<String>,
    #[serde(default)]
    pub(crate) reasoning: Option<String>,
    #[serde(default)]
    pub(crate) codex_fast_mode: Option<bool>,
}

impl ProjectModelsSet {
    fn keys(&self) -> Vec<ModelKey> {
        let mut keys = Vec::new();
        if self.primary.is_some() {
            keys.push(ModelKey::Primary);
        }
        if self.fast.is_some() {
            keys.push(ModelKey::Fast);
        }
        if self.compact.is_some() {
            keys.push(ModelKey::Compact);
        }
        if self.reasoning.is_some() {
            keys.push(ModelKey::Reasoning);
        }
        if self.codex_fast_mode.is_some() {
            keys.push(ModelKey::CodexFastMode);
        }
        keys
    }
}

/// 在原文上应用一次补丁:只增删 `[models]` 里的这几个键,注释、`[[permissions.rules]]`、
/// providers、scout 等其它内容原样保留;`[models]` 被删空就整张表移除。
/// `probe` 是校验模型值用的合并配置(provider 必须真的配了)。
pub(crate) fn apply_project_models_patch(
    text: &str,
    path: &Path,
    set: &ProjectModelsSet,
    unset: &[String],
    probe: &KanzeiConfig,
) -> Result<toml_edit::DocumentMut, String> {
    let mut unset_keys = Vec::new();
    for raw in unset {
        let key = ModelKey::from_camel(raw).ok_or_else(|| {
            format!("不支持的键 `{raw}`:项目模型配置只接受 primary/fast/compact/reasoning/codexFastMode")
        })?;
        unset_keys.push(key);
    }
    let set_keys = set.keys();
    if let Some(both) = set_keys.iter().find(|key| unset_keys.contains(key)) {
        return Err(format!(
            "`{}` 同时出现在 set 与 unset 里,不知道该写还是该删",
            both.camel()
        ));
    }
    let mut writes: Vec<(ModelKey, toml_edit::Value)> = Vec::new();
    for (key, value) in [
        (ModelKey::Primary, &set.primary),
        (ModelKey::Fast, &set.fast),
        (ModelKey::Compact, &set.compact),
    ] {
        let Some(value) = value else { continue };
        let spec = value.trim();
        if !spec.contains(':') {
            return Err(format!(
                "{} 的模型 `{spec}` 格式应为 provider:model",
                key.camel()
            ));
        }
        probe
            .resolve_model(spec)
            .map_err(|e| format!("{} 的模型 `{spec}` 无法解析:{e}", key.camel()))?;
        writes.push((key, spec.into()));
    }
    if let Some(reasoning) = &set.reasoning {
        let level = reasoning.trim().to_ascii_lowercase();
        if !REASONING_LEVELS.contains(&level.as_str()) {
            return Err(format!(
                "思考强度 `{reasoning}` 不是合法档位:{}",
                REASONING_LEVELS.join(" / ")
            ));
        }
        writes.push((ModelKey::Reasoning, level.into()));
    }
    if let Some(enabled) = set.codex_fast_mode {
        writes.push((ModelKey::CodexFastMode, enabled.into()));
    }
    let mut doc = crate::settings::settings_parse_document(text, path)?;
    let models = crate::settings::settings_table(&mut doc, "models")?;
    for (key, value) in writes {
        crate::settings::settings_set_value(models, key.snake(), value);
    }
    for key in unset_keys {
        models.remove(key.snake());
    }
    if models.is_empty() {
        doc.remove("models");
    }
    Ok(doc)
}

/// 读-改-写,写之前复读一次:自举 Agent 可能同时往同一个文件追加 `[[permissions.rules]]`
/// (「总是允许」),两次读之间文件变了就在新内容上重做,不覆盖别人刚追加的规则。
pub(crate) fn save_project_models_file(
    path: &Path,
    set: &ProjectModelsSet,
    unset: &[String],
    probe: &KanzeiConfig,
) -> Result<(), String> {
    save_project_models_file_with(path, set, unset, probe, || {})
}

/// `between_reads` 在「读 + 改」与「写前复读」之间调用——只给测试注入并发追加用。
fn save_project_models_file_with(
    path: &Path,
    set: &ProjectModelsSet,
    unset: &[String],
    probe: &KanzeiConfig,
    mut between_reads: impl FnMut(),
) -> Result<(), String> {
    for _ in 0..4 {
        let before = crate::settings::settings_read_text(path)?;
        let doc = apply_project_models_patch(
            before.as_deref().unwrap_or_default(),
            path,
            set,
            unset,
            probe,
        )?;
        if before.as_deref() == Some(doc.to_string().as_str()) {
            return Ok(()); // 没有实际变化:不碰文件
        }
        between_reads();
        if crate::settings::settings_read_text(path)? != before {
            continue; // 期间被别人改过:在新内容上重做
        }
        return crate::settings::settings_write_document(doc, path);
    }
    Err(format!(
        "{} 在保存期间被反复改写,为免覆盖别人的改动已放弃,请重试",
        path.display()
    ))
}

#[tauri::command]
pub(crate) fn project_models_save(
    project_dir: String,
    set: ProjectModelsSet,
    unset: Vec<String>,
) -> Result<Value, String> {
    let root = main_root(&project_dir);
    let probe = load_merged(Some(&root))?;
    save_project_models_file(&project_config_path(&root), &set, &unset, &probe)?;
    project_models_get_at(&project_dir)
}

/// 用系统默认程序打开项目配置文件。文件不存在只报错,不替用户建一个空文件。
#[tauri::command]
pub(crate) fn project_config_open(project_dir: String) -> Result<(), String> {
    let path = project_config_path(&main_root(&project_dir));
    if !path.is_file() {
        return Err(format!(
            "项目还没有配置文件:{}(保存一次项目模型配置会自动创建)",
            path.display()
        ));
    }
    crate::state::hidden_command("cmd")
        .args(["/c", "start", "", &path.display().to_string()])
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ---------- 设置页:哪些项目有自己的模型配置 ----------

fn present_keys(models: &ModelRoles) -> Vec<&'static str> {
    let mut keys = Vec::new();
    if nonblank(&models.primary).is_some() {
        keys.push("primary");
    }
    if nonblank(&models.fast).is_some() {
        keys.push("fast");
    }
    if nonblank(&models.compact).is_some() {
        keys.push("compact");
    }
    if nonblank(&models.reasoning).is_some() {
        keys.push("reasoning");
    }
    if models.codex_fast_mode.is_some() {
        keys.push("codexFastMode");
    }
    if nonblank(&models.scout).is_some() {
        keys.push("scout");
    }
    keys
}

/// 已登记项目里,项目文件 `[models]` 至少写了一个键的那些(它们不用设置页的全局默认)。
/// 同一主根只列一次;配置解析失败的项目跳过(那种项目开跑就会报错,不在这里重复)。
pub(crate) fn project_model_overrides(
    prefs: &crate::prefs::AppPrefs,
    current: Option<&str>,
) -> Vec<Value> {
    let current_root = current.map(main_root);
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for project in &prefs.projects {
        let root = main_root(project);
        if !seen.insert(root.clone()) {
            continue;
        }
        let path = project_config_path(&root);
        let Ok(models) = read_models_raw(&path) else {
            continue;
        };
        let keys = present_keys(&models);
        if keys.is_empty() {
            continue;
        }
        let name = prefs.names.get(project).cloned().unwrap_or_else(|| {
            root.file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| project.clone())
        });
        out.push(json!({
            "project": project,
            "name": name,
            "configPath": path.display().to_string(),
            "keys": keys,
            "current": current_root.as_ref() == Some(&root),
        }));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_harness::config::ProviderConfig;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-model-config-{tag}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn roles(
        primary: Option<&str>,
        fast: Option<&str>,
        reasoning: Option<&str>,
        codex_fast_mode: Option<bool>,
    ) -> ModelRoles {
        ModelRoles {
            primary: primary.map(str::to_string),
            fast: fast.map(str::to_string),
            reasoning: reasoning.map(str::to_string),
            codex_fast_mode,
            ..Default::default()
        }
    }

    fn openai_provider() -> ProviderConfig {
        ProviderConfig {
            protocol: "openai".into(),
            base_url: "http://127.0.0.1:1/v1".into(),
            api_key_env: None,
            api_key: None,
            auth: None,
            context_limit: Some(64_000),
        }
    }

    /// 测试用合并:与 config.rs merge 对 [models] 的规矩一致(项目层写了才覆盖),再 fill_defaults。
    fn merged_from(layers: &ModelLayers) -> KanzeiConfig {
        let mut config = KanzeiConfig::default();
        let pick = |p: &Option<String>, g: &Option<String>| p.clone().or_else(|| g.clone());
        config.models = ModelRoles {
            primary: pick(&layers.project.primary, &layers.global.primary),
            fast: pick(&layers.project.fast, &layers.global.fast),
            compact: pick(&layers.project.compact, &layers.global.compact),
            reasoning: pick(&layers.project.reasoning, &layers.global.reasoning),
            scout: pick(&layers.project.scout, &layers.global.scout),
            codex_fast_mode: layers
                .project
                .codex_fast_mode
                .or(layers.global.codex_fast_mode),
        };
        config.providers.insert("local".into(), openai_provider());
        config.fill_defaults();
        config
    }

    #[test]
    fn field_sources_project_global_builtin() {
        let layers = ModelLayers {
            global: roles(Some("codex:gpt-6-luna"), None, Some("xhigh"), None),
            project: roles(Some("codex:gpt-5.6-luna"), None, None, None),
        };
        let merged = merged_from(&layers);
        let primary = field_view(&layers, &merged, ModelKey::Primary);
        assert_eq!(primary.source, Source::Project);
        assert_eq!(primary.effective, json!("codex:gpt-5.6-luna"));
        assert_eq!(primary.inherited, json!("codex:gpt-6-luna"));
        assert_eq!(primary.inherited_source, Source::Global);
        let reasoning = field_view(&layers, &merged, ModelKey::Reasoning);
        assert_eq!(reasoning.source, Source::Global);
        assert_eq!(reasoning.project, Value::Null);
        assert_eq!(
            reasoning.inherited, reasoning.effective,
            "项目没写 → 继承值就是生效值"
        );
        assert_eq!(reasoning.effective, json!("xhigh"));

        let empty = ModelLayers::default();
        let merged = merged_from(&empty);
        let primary = field_view(&empty, &merged, ModelKey::Primary);
        assert_eq!(primary.source, Source::Builtin);
        assert_eq!(primary.inherited_source, Source::Builtin);
        assert_eq!(primary.inherited, primary.effective);
        assert_eq!(primary.effective, json!(builtin_primary()));
        let reasoning = field_view(&empty, &merged, ModelKey::Reasoning);
        assert_eq!(reasoning.source, Source::Builtin);
        assert_eq!(reasoning.effective, json!("off"));
    }

    #[test]
    fn fast_compact_follow_primary() {
        let layers = ModelLayers {
            global: roles(Some("codex:gpt-6-luna"), None, None, None),
            project: roles(Some("local:llama"), None, None, None),
        };
        let merged = merged_from(&layers);
        for key in [ModelKey::Fast, ModelKey::Compact] {
            let view = field_view(&layers, &merged, key);
            assert!(view.follows_primary, "{key:?} 两层都没写时应跟随 primary");
            assert!(view.inherited_follows_primary);
            assert_eq!(
                view.effective,
                json!("local:llama"),
                "跟随的是**合并后**的 primary"
            );
            assert_eq!(view.source, Source::Project, "来源取 primary 的来源");
        }
        // 全局写了 fast:不再跟随,来源是全局。
        let layers = ModelLayers {
            global: roles(Some("codex:gpt-6-luna"), Some("local:small"), None, None),
            project: roles(Some("local:llama"), None, None, None),
        };
        let merged = merged_from(&layers);
        let fast = field_view(&layers, &merged, ModelKey::Fast);
        assert!(!fast.follows_primary);
        assert_eq!(fast.effective, json!("local:small"));
        assert_eq!(fast.source, Source::Global);
        assert_eq!(merged.resolve_model("fast").unwrap().model, "small");
    }

    #[test]
    fn codex_fast_mode_builtin_rule() {
        let default_primary = builtin_primary();
        let layers = ModelLayers {
            global: roles(Some(&default_primary), None, None, None),
            project: ModelRoles::default(),
        };
        let merged = merged_from(&layers);
        let fast = field_view(&layers, &merged, ModelKey::CodexFastMode);
        assert_eq!(
            fast.effective,
            json!(true),
            "默认 Luna 且两层都没表态 → 内置开启"
        );
        assert_eq!(fast.source, Source::Builtin);
        assert_eq!(fast.inherited, json!(true));

        let layers = ModelLayers {
            global: roles(Some("codex:gpt-6-luna"), None, None, None),
            project: ModelRoles::default(),
        };
        let merged = merged_from(&layers);
        let fast = field_view(&layers, &merged, ModelKey::CodexFastMode);
        assert_eq!(
            fast.effective,
            json!(false),
            "primary 不是默认模型 → 内置不开"
        );
        assert_eq!(fast.source, Source::Builtin);
    }

    /// 用户现场(09-26 只读查询 state.db):全局 gpt-6-luna/xhigh,项目 gpt-5.6-luna/high,
    /// p20 线存了 codex:gpt-6-luna/xhigh。设置页说「实际生效 gpt-5.6-luna」,输入框却显示 gpt-6-luna。
    #[test]
    fn turn_view_reproduces_user_scene() {
        let layers = ModelLayers {
            global: roles(Some("codex:gpt-6-luna"), None, Some("xhigh"), Some(true)),
            project: roles(Some("codex:gpt-5.6-luna"), None, Some("high"), Some(true)),
        };
        let merged = merged_from(&layers);
        let view = turn_view(
            &merged,
            &layers,
            "primary",
            Some("codex:gpt-6-luna"),
            Some("xhigh"),
        );
        assert_eq!(view.model.resolved.as_deref(), Some("codex:gpt-6-luna"));
        assert_eq!(view.model.source, Source::Line);
        assert_eq!(
            view.default_model.resolved.as_deref(),
            Some("codex:gpt-5.6-luna")
        );
        assert_eq!(view.default_model.source, Source::Project);
        assert_eq!(view.default_model.role.as_deref(), Some("primary"));
        assert_eq!(view.reasoning.value, "xhigh");
        assert_eq!(view.reasoning.source, Source::Line);
        assert_eq!(view.default_reasoning.value, "high");
        assert_eq!(view.default_reasoning.source, Source::Project);
        assert!(view.codex_fast_mode.active);
        assert_eq!(view.codex_fast_mode.source, Source::Project);

        // 清掉本线覆盖(芯片「跟随默认」)→ 回到项目层。
        let view = turn_view(&merged, &layers, "primary", None, None);
        assert_eq!(view.model.source, Source::Project);
        assert_eq!(view.model.resolved.as_deref(), Some("codex:gpt-5.6-luna"));
        assert_eq!(view.model, view.default_model);
        assert_eq!(view.reasoning, view.default_reasoning);
    }

    /// 同源守护:芯片显示的就是运行路径真正会用的——模型走 resolve_model_chain+resolve_model,
    /// 思考档与 Fast mode 取自 build_runner_config 构造出来的 RunnerConfig。
    #[test]
    fn turn_view_matches_runner_path() {
        let global_luna = roles(
            Some("codex:gpt-6-luna"),
            Some("local:small"),
            Some("xhigh"),
            None,
        );
        let cases: Vec<(ModelLayers, &str, Option<&str>, Option<&str>)> = vec![
            (ModelLayers::default(), "primary", None, None),
            (
                ModelLayers {
                    global: global_luna.clone(),
                    project: roles(Some("codex:gpt-5.6-luna"), None, Some("high"), Some(true)),
                },
                "primary",
                Some("codex:gpt-6-luna"),
                Some("xhigh"),
            ),
            (
                ModelLayers {
                    global: global_luna.clone(),
                    project: ModelRoles::default(),
                },
                "fast",
                None,
                None,
            ),
            (
                ModelLayers {
                    global: global_luna.clone(),
                    project: roles(None, None, None, Some(true)),
                },
                "primary",
                Some("local:llama"),
                Some("low"),
            ),
            (
                ModelLayers {
                    global: roles(Some("local:llama"), None, None, Some(true)),
                    project: ModelRoles::default(),
                },
                "codex:gpt-6-sol",
                None,
                Some("max"),
            ),
            (
                ModelLayers {
                    global: global_luna,
                    project: roles(Some("local:llama"), None, Some("medium"), Some(false)),
                },
                "primary",
                Some("fast"),
                None,
            ),
        ];
        for (index, (layers, agent_model, line_model, line_reasoning)) in cases.iter().enumerate() {
            let merged = merged_from(layers);
            let view = turn_view(&merged, layers, agent_model, *line_model, *line_reasoning);
            // 运行路径:run_prompt → model_override = 本线模型 → assemble_run 的调用形态。
            let reference =
                kanzei_harness::config::resolve_model_chain(*line_model, None, agent_model);
            let resolved = merged.resolve_model(&reference).unwrap();
            let runner = kanzei_tools::run::build_runner_config(
                &resolved,
                &merged,
                *line_reasoning,
                Path::new("C:/kz-model-config-runner"),
                kanzei_core::AskPolicy::Interactive,
                None,
            );
            assert_eq!(
                view.model.resolved.as_deref(),
                Some(format!("{}:{}", resolved.provider_name, resolved.model).as_str()),
                "case {index}: 模型与运行路径不一致"
            );
            assert_eq!(
                view.reasoning.value,
                runner.reasoning.as_str(),
                "case {index}: 思考档与运行路径不一致"
            );
            assert_eq!(
                view.codex_fast_mode.enabled && view.codex_fast_mode.applies,
                runner.service_tier.is_some(),
                "case {index}: Fast mode 与运行路径不一致"
            );
            assert_eq!(view.codex_fast_mode.active, runner.service_tier.is_some());
            assert_eq!(view.context_limit, runner.context_limit, "case {index}");
        }
    }

    #[test]
    fn turn_view_reports_resolve_error() {
        let layers = ModelLayers::default();
        let merged = merged_from(&layers);
        let view = turn_view(&merged, &layers, "primary", Some("nope:model"), None);
        assert_eq!(view.model.resolved, None);
        assert!(
            view.model
                .error
                .as_deref()
                .is_some_and(|e| e.contains("nope")),
            "{:?}",
            view.model.error
        );
        assert_eq!(view.model.source, Source::Line);
        assert!(!view.codex_fast_mode.active);
        assert!(
            view.default_model.resolved.is_some(),
            "默认那一项不受本线坏值影响"
        );
    }

    fn probe_with_local() -> KanzeiConfig {
        let mut probe = KanzeiConfig::default();
        probe.providers.insert("local".into(), openai_provider());
        probe.fill_defaults();
        probe
    }

    const PROJECT_TOML: &str = "# 项目配置:手写注释\n\
        [models]\n\
        scout = \"local:scout\" # 勘察\n\
        fast = \"local:small\"\n\
        \n\
        [providers.local]\n\
        protocol = \"openai\"\n\
        base_url = \"http://127.0.0.1:1/v1\"\n\
        \n\
        [[permissions.rules]]\n\
        action = \"bash\"\n\
        resource = \"git status\"\n\
        effect = \"allow\"\n";

    #[test]
    fn project_models_save_writes_only_models_keys_and_preserves_rest() {
        let dir = temp_dir("save");
        let path = dir.join(".kanzei/kanzei.toml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, PROJECT_TOML).unwrap();
        let set = ProjectModelsSet {
            primary: Some("local:llama".into()),
            reasoning: Some("HIGH".into()),
            codex_fast_mode: Some(false),
            ..Default::default()
        };
        save_project_models_file(&path, &set, &["fast".to_string()], &probe_with_local()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        for kept in [
            "# 项目配置:手写注释",
            "scout = \"local:scout\" # 勘察",
            "[providers.local]",
            "[[permissions.rules]]",
            "resource = \"git status\"",
        ] {
            assert!(text.contains(kept), "丢了 `{kept}`:\n{text}");
        }
        let config: KanzeiConfig = toml::from_str(&text).unwrap();
        assert_eq!(config.models.primary.as_deref(), Some("local:llama"));
        assert_eq!(config.models.reasoning.as_deref(), Some("high"));
        assert_eq!(config.models.codex_fast_mode, Some(false));
        assert_eq!(config.models.fast, None, "unset 的键要真的删掉");
        assert_eq!(config.models.compact, None, "没动的键不能被写出来");
        assert_eq!(config.permissions.rules.len(), 1);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn project_models_save_unset_all_removes_empty_models_table() {
        let dir = temp_dir("unset");
        let path = dir.join("kanzei.toml");
        std::fs::write(
            &path,
            "[models]\nprimary = \"local:llama\"\nfast = \"local:small\"\nreasoning = \"high\"\ncodex_fast_mode = true\n\n[providers.local]\nprotocol = \"openai\"\nbase_url = \"http://x\"\n",
        )
        .unwrap();
        let all: Vec<String> = ModelKey::ALL
            .iter()
            .map(|k| k.camel().to_string())
            .collect();
        save_project_models_file(
            &path,
            &ProjectModelsSet::default(),
            &all,
            &probe_with_local(),
        )
        .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            !text.contains("[models]"),
            "[models] 删空后应整张表移除:\n{text}"
        );
        assert!(text.contains("[providers.local]"));

        // 还剩 scout(本弹窗不管的键)→ 表保留。
        std::fs::write(&path, PROJECT_TOML).unwrap();
        save_project_models_file(
            &path,
            &ProjectModelsSet::default(),
            &all,
            &probe_with_local(),
        )
        .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("[models]") && text.contains("scout = \"local:scout\""),
            "{text}"
        );
        assert!(!text.contains("fast = "), "{text}");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn project_models_save_rejects_invalid() {
        let dir = temp_dir("reject");
        let path = dir.join("kanzei.toml");
        std::fs::write(&path, PROJECT_TOML).unwrap();
        let probe = probe_with_local();
        let cases: Vec<(ProjectModelsSet, Vec<String>, &str)> = vec![
            (
                ProjectModelsSet {
                    primary: Some("ghost:model".into()),
                    ..Default::default()
                },
                vec![],
                "ghost",
            ),
            (
                ProjectModelsSet {
                    primary: Some("primary".into()),
                    ..Default::default()
                },
                vec![],
                "provider:model",
            ),
            (
                ProjectModelsSet {
                    reasoning: Some("turbo".into()),
                    ..Default::default()
                },
                vec![],
                "turbo",
            ),
            (ProjectModelsSet::default(), vec!["proxy".into()], "proxy"),
            (
                ProjectModelsSet {
                    fast: Some("local:small".into()),
                    ..Default::default()
                },
                vec!["fast".into()],
                "fast",
            ),
        ];
        for (set, unset, needle) in cases {
            let error = save_project_models_file(&path, &set, &unset, &probe).unwrap_err();
            assert!(error.contains(needle), "报错要点名 `{needle}`:{error}");
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                PROJECT_TOML,
                "拒绝时文件必须一字不动"
            );
        }
        // 载荷里带模型之外的键(例如 proxy / providers)在反序列化时就被拒。
        assert!(
            serde_json::from_value::<ProjectModelsSet>(json!({ "proxy": "http://x" })).is_err()
        );
        assert!(serde_json::from_value::<ProjectModelsSet>(
            json!({ "primary": "local:llama", "codexFastMode": true })
        )
        .is_ok());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn project_models_save_rebases_on_concurrent_append() {
        // 两次读之间有人追加了一条「总是允许」规则:在新内容上重做,规则不能丢。
        let root = temp_dir("rebase");
        let path = root.join(".kanzei/kanzei.toml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "[models]\nprimary = \"local:llama\"\n").unwrap();
        let set = ProjectModelsSet {
            reasoning: Some("low".into()),
            ..Default::default()
        };
        let mut appended = false;
        save_project_models_file_with(&path, &set, &[], &probe_with_local(), || {
            // 第一次读完、写之前,自举 Agent 的「总是允许」追加了一条规则(真实持久化函数)。
            if !appended {
                appended = true;
                kanzei_harness::config::append_allow_rule(&root, "bash", "cargo test").unwrap();
            }
        })
        .unwrap();
        assert!(appended);
        let text = std::fs::read_to_string(&path).unwrap();
        let config: KanzeiConfig = toml::from_str(&text).unwrap();
        assert_eq!(
            config.permissions.rules.len(),
            1,
            "并发追加的规则被覆盖:\n{text}"
        );
        assert_eq!(config.models.reasoning.as_deref(), Some("low"), "{text}");
        assert_eq!(config.models.primary.as_deref(), Some("local:llama"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn project_model_overrides_lists_only_projects_with_keys() {
        let dir = temp_dir("overrides");
        let with = dir.join("with-models");
        let without = dir.join("without-models");
        for project in [&with, &without] {
            std::fs::create_dir_all(project.join(".kanzei")).unwrap();
        }
        std::fs::write(
            with.join(".kanzei/kanzei.toml"),
            "[models]\nprimary = \"codex:gpt-5.6-luna\"\nreasoning = \"high\"\ncodex_fast_mode = true\n",
        )
        .unwrap();
        std::fs::write(
            without.join(".kanzei/kanzei.toml"),
            "[[permissions.rules]]\naction = \"read\"\nresource = \"*\"\neffect = \"allow\"\n",
        )
        .unwrap();
        let with_s = with.display().to_string();
        let without_s = without.display().to_string();
        let prefs = crate::prefs::AppPrefs {
            projects: vec![with_s.clone(), without_s.clone(), with_s.clone()],
            names: [(with_s.clone(), "带模型的项目".to_string())]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        let list = project_model_overrides(&prefs, Some(&with_s));
        assert_eq!(
            list.len(),
            1,
            "只列有 [models] 键的项目,同一项目只列一次:{list:?}"
        );
        assert_eq!(list[0]["project"], json!(with_s));
        assert_eq!(list[0]["name"], json!("带模型的项目"));
        assert_eq!(
            list[0]["keys"],
            json!(["primary", "reasoning", "codexFastMode"])
        );
        assert_eq!(list[0]["current"], json!(true));
        let list = project_model_overrides(&prefs, Some(&without_s));
        assert_eq!(list[0]["current"], json!(false));
        std::fs::remove_dir_all(dir).ok();
    }

    /// D-381 同款形状契约:model_effective 的键结构与 scripts/ipc-contract.json 一致,
    /// ui-runtime-smoke 用同一份契约校验它的桩。有意改形状:
    /// `KZ_UPDATE_IPC_CONTRACT=1 cargo test -p kanzei-app model_effective_形状` 写回。
    #[test]
    fn model_effective_形状与ipc契约一致() {
        let layers = ModelLayers {
            global: roles(Some("codex:gpt-6-luna"), None, Some("xhigh"), None),
            project: roles(Some("codex:gpt-5.6-luna"), None, Some("high"), Some(true)),
        };
        let merged = merged_from(&layers);
        let mut view = turn_view(&merged, &layers, "primary", Some("codex:gpt-6-luna"), None);
        view.agent = Some("dev-pair".into());
        let actual = crate::ipc_contract::shape(&serde_json::to_value(view).unwrap());
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/ipc-contract.json");
        let mut all: Value = serde_json::from_str(
            &std::fs::read_to_string(&path).expect("读不到 ipc-contract.json"),
        )
        .expect("ipc-contract.json 不是合法 JSON");
        if std::env::var("KZ_UPDATE_IPC_CONTRACT").is_ok() {
            all["model_effective"] = actual;
            std::fs::write(&path, serde_json::to_string_pretty(&all).unwrap() + "\n").unwrap();
            return;
        }
        assert_eq!(
            Some(&actual),
            all.get("model_effective"),
            "model_effective 的 IPC 形状变了:同步 scripts/ipc-contract.json + ui-runtime-smoke 的桩 + 读它的 ui/08-*.js。\n实际:{}",
            serde_json::to_string_pretty(&actual).unwrap_or_default()
        );
    }
}
