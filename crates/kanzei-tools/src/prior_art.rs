//! R-248:新方向开工前的先行方案对照。
//!
//! 这里仅管理触发、工件骨架、证据形状与联网次数；研究计划、来源/发现和写作
//! 仍由既有 research 工具负责，避免再造第二套研究状态机。

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::docstore::DocStore;

pub const PRIOR_ART_FILE: &str = "prior-art.md";
const SEARCH_STATE_FILE: &str = "prior-art-search.json";
pub const MAX_PRIOR_ART_SEARCH_ROUNDS: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriorArtTrigger {
    ProjectInit,
    CoreRequirement,
    ExplicitUser,
}

impl PriorArtTrigger {
    fn as_str(self) -> &'static str {
        match self {
            Self::ProjectInit => "project_init",
            Self::CoreRequirement => "core_requirement",
            Self::ExplicitUser => "explicit_user",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriorArtStart {
    pub relative_path: String,
    pub absolute_path: PathBuf,
    pub created: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriorArtValidation {
    pub topic: String,
    pub external_count: usize,
    pub internal_count: usize,
    pub search_round_limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PriorArtSearchState {
    used: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    External,
    Internal,
}

#[derive(Debug, Default)]
struct Conclusion {
    side: Option<Side>,
    name: String,
    fields: BTreeMap<String, String>,
}

fn metadata(text: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return fields;
    }
    for line in lines {
        let line = line.trim();
        if line == "---" {
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            fields.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    fields
}

fn split_field(line: &str) -> Option<(&str, &str)> {
    let line = line.trim().strip_prefix('-')?.trim();
    line.split_once(':')
        .or_else(|| line.split_once('：'))
        .map(|(key, value)| (key.trim(), value.trim()))
}

fn conclusions(text: &str) -> Vec<Conclusion> {
    let mut side = None;
    let mut current: Option<Conclusion> = None;
    let mut found = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        match trimmed {
            "## 外部已有实现" => {
                if let Some(item) = current.take() {
                    found.push(item);
                }
                side = Some(Side::External);
            }
            "## 仓内既有设计" => {
                if let Some(item) = current.take() {
                    found.push(item);
                }
                side = Some(Side::Internal);
            }
            _ if trimmed.starts_with("## ") => {
                if let Some(item) = current.take() {
                    found.push(item);
                }
                side = None;
            }
            _ if trimmed.starts_with("### ") && side.is_some() => {
                if let Some(item) = current.take() {
                    found.push(item);
                }
                current = Some(Conclusion {
                    side,
                    name: trimmed.trim_start_matches("### ").trim().to_string(),
                    fields: BTreeMap::new(),
                });
            }
            _ => {
                if let (Some(item), Some((key, value))) = (current.as_mut(), split_field(trimmed)) {
                    item.fields.insert(key.to_string(), value.to_string());
                }
            }
        }
    }
    if let Some(item) = current {
        found.push(item);
    }
    found
}

fn safe_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

fn expected_prior_art_path(root: &Path, relative: &str) -> Result<(String, PathBuf), String> {
    let relative_path = Path::new(relative);
    if !safe_relative(relative_path) {
        return Err("prior_art 必须是项目内相对路径，且不能包含 ..".into());
    }
    let components = relative_path
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>();
    if components.len() != 4
        || components[0] != ".kanzei"
        || components[1] != "research"
        || components[3] != PRIOR_ART_FILE
    {
        return Err("prior_art 路径必须精确为 `.kanzei/research/<topic>/prior-art.md`".into());
    }
    DocStore::validate_topic(components[2]).map_err(|error| error.to_string())?;
    Ok((components[2].to_string(), root.join(relative_path)))
}

fn validate_url(source: &str) -> bool {
    (source.starts_with("https://") || source.starts_with("http://"))
        && source.len() > "https://".len()
        && !source.chars().any(char::is_whitespace)
}

fn validate_file_anchor(root: &Path, source: &str) -> Result<(), String> {
    let Some(anchor) = source.strip_prefix("file:") else {
        return Err("仓内结论的出处必须使用 `file:相对路径:行号`".into());
    };
    let Some((raw_path, raw_line)) = anchor.rsplit_once(':') else {
        return Err("仓内结论的出处缺少行号，应为 `file:相对路径:行号`".into());
    };
    let line = raw_line
        .parse::<usize>()
        .ok()
        .filter(|line| *line > 0)
        .ok_or_else(|| "仓内结论的出处行号必须是正整数".to_string())?;
    let path = Path::new(raw_path);
    if !safe_relative(path) {
        return Err("仓内出处必须是项目内相对路径，且不能包含 ..".into());
    }
    let absolute = root.join(path);
    let text = std::fs::read_to_string(&absolute)
        .map_err(|error| format!("仓内出处不可读 `{}`: {error}", path.display()))?;
    if line > text.lines().count().max(1) {
        return Err(format!(
            "仓内出处 `{}` 只有 {} 行，锚点第 {line} 行不存在",
            path.display(),
            text.lines().count()
        ));
    }
    Ok(())
}

fn validate_conclusion(root: &Path, conclusion: &Conclusion) -> Result<(), String> {
    if conclusion.name.trim().is_empty() {
        return Err("prior-art 结论的方案名不能为空".into());
    }
    let source = conclusion
        .fields
        .get("出处")
        .map(String::as_str)
        .unwrap_or("");
    let evidence_level = conclusion
        .fields
        .get("证据等级")
        .map(String::as_str)
        .unwrap_or("");
    let difference = conclusion
        .fields
        .get("差异")
        .map(String::as_str)
        .unwrap_or("");
    let decision = conclusion
        .fields
        .get("决策")
        .map(String::as_str)
        .unwrap_or("");
    if source.is_empty() || difference.is_empty() || decision.is_empty() {
        return Err(format!(
            "方案 `{}` 必须同时填写出处、差异、决策，不能用无证据结论通过门禁",
            conclusion.name
        ));
    }
    if !matches!(evidence_level, "V0" | "V1" | "V2" | "V3") {
        return Err(format!(
            "方案 `{}` 的证据等级必须是 V0/V1/V2/V3",
            conclusion.name
        ));
    }
    match conclusion.side {
        Some(Side::External) if !validate_url(source) => Err(format!(
            "外部方案 `{}` 的出处必须是 http(s) URL",
            conclusion.name
        )),
        Some(Side::Internal) => validate_file_anchor(root, source)
            .map_err(|error| format!("仓内方案 `{}` 的出处无效: {error}", conclusion.name)),
        Some(Side::External) => Ok(()),
        None => Err(format!("方案 `{}` 不在受控双侧章节内", conclusion.name)),
    }
}

pub fn validate_artifact(
    root: &Path,
    relative: &str,
    expected_entry_ref: Option<&str>,
) -> Result<PriorArtValidation, String> {
    let (topic, path) = expected_prior_art_path(root, relative)?;
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("读取 prior-art 工件失败 `{relative}`: {error}"))?;
    let meta = metadata(&text);
    if meta.get("kind").map(String::as_str) != Some("prior_art") {
        return Err("prior-art 工件头必须声明 `kind: prior_art`".into());
    }
    if meta.get("topic").map(String::as_str) != Some(topic.as_str()) {
        return Err(format!(
            "prior-art 工件头的 topic 必须与目录 `{topic}` 一致"
        ));
    }
    if meta.get("status").map(String::as_str) != Some("complete") {
        return Err("prior-art 工件尚未完成：将双侧对照补齐后把 `status` 改为 `complete`".into());
    }
    if let Some(expected) = expected_entry_ref {
        let refs = meta.get("entry_refs").map(String::as_str).unwrap_or("");
        if !refs.split_whitespace().any(|value| value == expected) {
            return Err(format!(
                "prior-art 工件头 `entry_refs` 必须包含本次待登记编号 {expected}"
            ));
        }
    }
    let search_round_limit = meta
        .get("websearch_round_limit")
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| (1..=MAX_PRIOR_ART_SEARCH_ROUNDS).contains(value))
        .ok_or_else(|| {
            format!("websearch_round_limit 必须是 1..={MAX_PRIOR_ART_SEARCH_ROUNDS} 的整数")
        })?;
    let conclusions = conclusions(&text);
    for conclusion in &conclusions {
        validate_conclusion(root, conclusion)?;
    }
    let external_count = conclusions
        .iter()
        .filter(|conclusion| conclusion.side == Some(Side::External))
        .count();
    let internal_count = conclusions
        .iter()
        .filter(|conclusion| conclusion.side == Some(Side::Internal))
        .count();
    if external_count == 0 || internal_count == 0 {
        return Err(format!(
            "prior-art 双侧覆盖不足：外部已有实现 {external_count} 条，仓内既有设计 {internal_count} 条；两侧都至少需要一条"
        ));
    }
    Ok(PriorArtValidation {
        topic,
        external_count,
        internal_count,
        search_round_limit,
    })
}

pub fn requirement_topic(id: &str, title: &str) -> String {
    let mut slug = String::new();
    let mut separator = false;
    for character in title.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            if separator && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(character);
            separator = false;
        } else {
            separator = true;
        }
        if slug.len() >= 36 {
            break;
        }
    }
    let id = id.to_ascii_lowercase().replace('-', "");
    if slug.is_empty() {
        format!("{id}-prior-art")
    } else {
        format!("{id}-{slug}")
    }
}

pub fn start_scaffold(
    root: &Path,
    topic: &str,
    trigger: PriorArtTrigger,
    entry_ref: Option<&str>,
) -> Result<PriorArtStart, String> {
    DocStore::validate_topic(topic).map_err(|error| error.to_string())?;
    let relative_path = format!(".kanzei/research/{topic}/{PRIOR_ART_FILE}");
    let absolute_path = root.join(&relative_path);
    if absolute_path.is_file() {
        return Ok(PriorArtStart {
            relative_path,
            absolute_path,
            created: false,
        });
    }
    let parent = absolute_path
        .parent()
        .ok_or_else(|| "prior-art 路径缺少父目录".to_string())?;
    std::fs::create_dir_all(parent).map_err(|error| format!("创建 prior-art 目录失败: {error}"))?;
    let entry_refs = entry_ref.unwrap_or("");
    let content = format!(
        "---\nkind: prior_art\ntopic: {topic}\nstatus: pending\ntrigger: {}\nentry_refs: {entry_refs}\nwebsearch_round_limit: {MAX_PRIOR_ART_SEARCH_ROUNDS}\n---\n\n# 先行方案对照\n\n> 完成前请补齐双侧至少各一条对照，并将 `status` 改为 `complete`。\n\n## 外部已有实现\n\n<!-- 每条使用：### 方案名；- 出处: https://...；- 证据等级: V0..V3；- 差异: ...；- 决策: 采用/不采用及理由 -->\n\n## 仓内既有设计\n\n<!-- 每条使用：### 设计名；- 出处: file:docs/design/example.md:1；- 证据等级: V0..V3；- 差异: ...；- 决策: 采用/不采用及理由 -->\n",
        trigger.as_str()
    );
    kanzei_base::atomic_file::write_atomic(&absolute_path, &content)
        .map_err(|error| format!("写入 prior-art 骨架失败: {error}"))?;
    Ok(PriorArtStart {
        relative_path,
        absolute_path,
        created: true,
    })
}

pub fn start_project_init(root: &Path) -> Result<PriorArtStart, String> {
    start_scaffold(root, "project-init", PriorArtTrigger::ProjectInit, None)
}

/// tracker requirement 登记时传入的先行调研判定材料。
///
/// R-248 的策略属于 prior-art 领域；tracker 只负责提供登记字段与接收审计字段，
/// 避免把触发、工件和豁免规则重新堆回通用 CRUD。
pub(crate) struct RegistrationCheck<'a> {
    pub requirement: bool,
    pub fields: &'a BTreeMap<String, String>,
    pub refs_empty: bool,
    pub artifact: Option<&'a str>,
    pub waiver: Option<&'a str>,
    pub id: &'a str,
    pub title: &'a str,
}

pub(crate) fn check_registration(
    ctx: &ToolCtx,
    input: RegistrationCheck<'_>,
) -> Result<Option<(String, String)>, String> {
    if !input.requirement {
        return Ok(None);
    }
    if input.artifact.is_some() && input.waiver.is_some() {
        return Err(
            "prior_art 与 prior_art_waiver 互斥：要么提交工件，要么记录用户豁免理由".into(),
        );
    }
    let core = input.fields.iter().any(|(key, value)| {
        (key.as_str() == "标签"
            || key.eq_ignore_ascii_case("tags")
            || key.eq_ignore_ascii_case("tag"))
            && value
                .split(|character: char| character == ',' || character.is_whitespace())
                .any(|tag| tag == "核心")
    });
    let triggered = core && input.refs_empty;
    if let Some(relative) = input
        .artifact
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        validate_artifact(&ctx.project_root, relative, Some(input.id))?;
        return Ok(Some(("先行调研".into(), relative.into())));
    }
    if let Some(reason) = input
        .waiver
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if !triggered {
            return Err("prior_art_waiver 只用于「核心 + refs 为空」的新方向登记".into());
        }
        if reason.chars().count() < 8 {
            return Err(
                "prior_art_waiver 必须记录至少 8 个字符的明确用户理由，不能写空泛占位".into(),
            );
        }
        return Ok(Some(("先行调研豁免".into(), reason.into())));
    }
    if !triggered {
        return Ok(None);
    }

    let topic = requirement_topic(input.id, input.title);
    let start = start_scaffold(
        &ctx.project_root,
        &topic,
        PriorArtTrigger::CoreRequirement,
        Some(input.id),
    )?;
    if start.created {
        crate::record_write_log(ctx, &start.relative_path, &start.absolute_path);
    }
    Err(format!(
        "CORE_REQUIREMENT_PRIOR_ART_REQUIRED: 核心 requirement 且 refs 为空，已机械判定为新方向并创建 `{}`。先补齐外部已有实现与仓内既有设计，将 status 改为 complete，再以 top-level prior_art 重试；若用户明确决定跳过，改传 prior_art_waiver 并写明理由。refs 仍只写 R-/D-/T-，不要把文件路径塞进 refs。",
        start.relative_path
    ))
}

pub fn consume_search_round(root: &Path, topic: &str) -> Result<(u32, u32), String> {
    DocStore::validate_topic(topic).map_err(|error| error.to_string())?;
    let dir = root.join(".kanzei/research").join(topic);
    let prior_art = dir.join(PRIOR_ART_FILE);
    let text = std::fs::read_to_string(&prior_art).map_err(|error| {
        format!("topic `{topic}` 没有可读的 prior-art.md，不能登记先行调研 websearch 轮次: {error}")
    })?;
    let limit = metadata(&text)
        .get("websearch_round_limit")
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| (1..=MAX_PRIOR_ART_SEARCH_ROUNDS).contains(value))
        .ok_or_else(|| {
            format!(
                "prior-art.md 的 websearch_round_limit 必须是 1..={MAX_PRIOR_ART_SEARCH_ROUNDS}"
            )
        })?;
    let state_path = dir.join(SEARCH_STATE_FILE);
    let _lock = kanzei_base::atomic_file::lock_exclusive(&state_path)
        .map_err(|error| format!("锁定 prior-art 搜索预算失败: {error}"))?;
    let mut state = std::fs::read_to_string(&state_path)
        .ok()
        .and_then(|raw| serde_json::from_str::<PriorArtSearchState>(&raw).ok())
        .unwrap_or_default();
    if state.used >= limit {
        return Err(format!(
            "PRIOR_ART_SEARCH_LIMIT: topic `{topic}` 已使用 {}/{limit} 轮 websearch；预算已耗尽。请基于现有来源完成对照，或由用户调整工件预算（上限 {MAX_PRIOR_ART_SEARCH_ROUNDS}），不能静默继续搜索。",
            state.used
        ));
    }
    state.used += 1;
    let raw = serde_json::to_string_pretty(&state)
        .map_err(|error| format!("序列化 prior-art 搜索预算失败: {error}"))?;
    kanzei_base::atomic_file::write_atomic(&state_path, &raw)
        .map_err(|error| format!("保存 prior-art 搜索预算失败: {error}"))?;
    Ok((state.used, limit))
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PriorArtInput {
    /// start=用户显式发起并创建骨架；validate=机械核验已有工件。
    action: String,
    /// 小写 kebab-case，对应 `.kanzei/research/<topic>/`。
    topic: String,
    /// validate 时可选：要求工件 entry_refs 包含此 R-/D-/T-。
    #[serde(default)]
    entry_ref: Option<String>,
}

pub struct PriorArtTool;

#[async_trait]
impl Tool for PriorArtTool {
    fn name(&self) -> &'static str {
        "prior_art"
    }

    fn description(&self) -> String {
        "先行方案对照：start 在用户显式要求时创建 `.kanzei/research/<topic>/prior-art.md` 骨架；validate 机械检查每条结论的出处/V级/差异/决策，以及外部实现与仓内设计双侧覆盖。".into()
    }

    fn input_schema(&self) -> serde_json::Value {
        let mut schema = serde_json::to_value(schemars::schema_for!(PriorArtInput)).unwrap();
        schema["properties"]["action"] =
            serde_json::json!({ "type": "string", "enum": ["start", "validate"] });
        schema["properties"]["topic"]["pattern"] = serde_json::json!("^[a-z0-9]+(?:-[a-z0-9]+)*$");
        schema
    }

    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        let action = input
            .get("action")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        vec![format!(
            "{}:{action}",
            if action == "validate" {
                "read"
            } else {
                "write"
            }
        )]
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: PriorArtInput = match crate::parse_input(self, input) {
            Ok(value) => value,
            Err(output) => return output,
        };
        match input.action.as_str() {
            "start" => match start_scaffold(
                &ctx.project_root,
                &input.topic,
                PriorArtTrigger::ExplicitUser,
                input.entry_ref.as_deref(),
            ) {
                Ok(start) => {
                    if start.created {
                        crate::record_write_log(ctx, &start.relative_path, &start.absolute_path);
                    }
                    ToolOutput::ok(
                        serde_json::json!({
                            "trigger": "explicit_user",
                            "path": start.relative_path,
                            "created": start.created,
                            "status": "pending"
                        })
                        .to_string(),
                    )
                }
                Err(error) => ToolOutput::error(error),
            },
            "validate" => {
                let relative = format!(".kanzei/research/{}/{}", input.topic, PRIOR_ART_FILE);
                match validate_artifact(&ctx.project_root, &relative, input.entry_ref.as_deref()) {
                    Ok(validation) => ToolOutput::ok(
                        serde_json::json!({
                            "path": relative,
                            "topic": validation.topic,
                            "external_count": validation.external_count,
                            "internal_count": validation.internal_count,
                            "websearch_round_limit": validation.search_round_limit,
                            "valid": true
                        })
                        .to_string(),
                    ),
                    Err(error) => ToolOutput::needs_correction("INVALID_PRIOR_ART", error),
                }
            }
            _ => ToolOutput::needs_correction(
                "INVALID_ACTION",
                "prior_art action 只能是 start/validate",
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kz-prior-art-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("docs/design")).unwrap();
        std::fs::write(root.join("docs/design/base.md"), "# 基线\n现有设计\n").unwrap();
        root
    }

    fn complete_artifact(root: &Path, topic: &str, entry_ref: &str) -> String {
        let start = start_scaffold(
            root,
            topic,
            PriorArtTrigger::CoreRequirement,
            Some(entry_ref),
        )
        .unwrap();
        let text = format!(
            "---\nkind: prior_art\ntopic: {topic}\nstatus: complete\ntrigger: core_requirement\nentry_refs: {entry_ref}\nwebsearch_round_limit: 2\n---\n\n# 先行方案对照\n\n## 外部已有实现\n\n### upstream\n- 出处: https://example.test/design\n- 证据等级: V1\n- 差异: 上游只覆盖单机\n- 决策: 采用其数据结构\n\n## 仓内既有设计\n\n### current design\n- 出处: file:docs/design/base.md:2\n- 证据等级: V2\n- 差异: 当前缺少门禁\n- 决策: 保留现有状态机并补门禁\n"
        );
        std::fs::write(&start.absolute_path, text).unwrap();
        start.relative_path
    }

    #[test]
    fn 三种触发创建同一受控骨架形状() {
        let root = root("triggers");
        let project = start_project_init(&root).unwrap();
        let core = start_scaffold(
            &root,
            "r001-core",
            PriorArtTrigger::CoreRequirement,
            Some("R-001"),
        )
        .unwrap();
        let explicit =
            start_scaffold(&root, "user-requested", PriorArtTrigger::ExplicitUser, None).unwrap();
        for (start, trigger) in [
            (project, "trigger: project_init"),
            (core, "trigger: core_requirement"),
            (explicit, "trigger: explicit_user"),
        ] {
            let text = std::fs::read_to_string(start.absolute_path).unwrap();
            assert!(text.contains(trigger));
            assert!(text.contains("status: pending"));
            assert!(text.contains("## 外部已有实现"));
            assert!(text.contains("## 仓内既有设计"));
        }
    }

    #[test]
    fn 双侧完整工件通过且任一侧缺失或无出处都拒绝() {
        let root = root("validation");
        let path = complete_artifact(&root, "r001-core", "R-001");
        let valid = validate_artifact(&root, &path, Some("R-001")).unwrap();
        assert_eq!(valid.external_count, 1);
        assert_eq!(valid.internal_count, 1);

        let absolute = root.join(&path);
        let complete = std::fs::read_to_string(&absolute).unwrap();
        std::fs::write(
            &absolute,
            complete.replace("- 出处: https://example.test/design\n", ""),
        )
        .unwrap();
        assert!(validate_artifact(&root, &path, Some("R-001"))
            .unwrap_err()
            .contains("出处"));

        std::fs::write(&absolute, complete.split("## 仓内既有设计").next().unwrap()).unwrap();
        assert!(validate_artifact(&root, &path, Some("R-001"))
            .unwrap_err()
            .contains("双侧覆盖不足"));
    }

    #[test]
    fn websearch轮次达到上限后给明确诊断() {
        let root = root("budget");
        let _ = complete_artifact(&root, "r001-core", "R-001");
        assert_eq!(consume_search_round(&root, "r001-core").unwrap(), (1, 2));
        assert_eq!(consume_search_round(&root, "r001-core").unwrap(), (2, 2));
        let error = consume_search_round(&root, "r001-core").unwrap_err();
        assert!(error.contains("PRIOR_ART_SEARCH_LIMIT"));
        assert!(error.contains("2/2"));
    }

    #[test]
    fn requirement_topic始终生成合法且有编号归属的topic() {
        assert_eq!(
            requirement_topic("R-248", "Prior Art / 先行调研"),
            "r248-prior-art"
        );
        assert_eq!(requirement_topic("R-318", "全中文标题"), "r318-prior-art");
    }
}
