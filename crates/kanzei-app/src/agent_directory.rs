//! Agent 目录只读入口(R-305 B1)。
//!
//! 运行时 Agent 仍由 Harness 解析；本模块只把同一套全局/项目 Markdown 来源与
//! 内建注册表投影给设置页，并把配置错误显式呈现，不提供编辑或启停旁路。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use kanzei_harness::defs::{AgentMode, ProfileKind, ProfileScope, DEFAULT_AGENT_STEPS};
use kanzei_harness::markdown::parse_frontmatter;
use kanzei_harness::{AgentDef, KanzeiConfig, ResolveCtx};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentDirectoryEntry {
    pub(crate) name: String,
    pub(crate) source: String,
    pub(crate) path: Option<String>,
    pub(crate) profile: String,
    pub(crate) mode: String,
    pub(crate) model: String,
    pub(crate) steps: u32,
    pub(crate) status: String,
    pub(crate) system_preview: String,
    pub(crate) error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentDirectoryResponse {
    pub(crate) profile: String,
    pub(crate) agents: Vec<AgentDirectoryEntry>,
}

struct FileAgent {
    name: String,
    entry: AgentDirectoryEntry,
    scope: Option<ProfileScope>,
}

#[tauri::command]
pub(crate) fn agent_directory_get(
    project_dir: Option<String>,
    profile: Option<String>,
) -> Result<AgentDirectoryResponse, String> {
    let cwd = project_dir
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let project_root = kanzei_harness::config::discover_project_root(&cwd).unwrap_or(cwd);
    let (config, _) = KanzeiConfig::load_with_warnings_at_root(&project_root)
        .map_err(|e| format!("读取配置失败: {e}"))?;
    let profile_name = profile
        .or_else(|| config.profile.default.clone())
        .unwrap_or_else(|| "dev".into());
    let profile_kind: ProfileKind = profile_name
        .parse()
        .map_err(|e: String| format!("Agent 目录档位无效: {e}"))?;

    let mut files = BTreeMap::<String, FileAgent>::new();
    if let Some(home) = kanzei_harness::kanzei_home() {
        scan_agent_files(&home.join("agents"), "global", &mut files);
    }
    scan_agent_files(
        &project_root.join(".kanzei").join("agents"),
        "project",
        &mut files,
    );

    let ctx = ResolveCtx {
        profile: profile_kind,
        cwd: project_root.clone(),
        project_root: project_root.clone(),
        config: std::sync::Arc::new(config),
    };
    let harness = crate::run::assembly::build_run_harness(false, None);
    let snapshot = harness
        .resolve(&ctx)
        .map_err(|e| format!("解析内建 Agent 失败: {e}"))?;
    for (_, agent) in snapshot.agents().iter() {
        files
            .entry(agent.name.clone())
            .or_insert_with(|| FileAgent {
                name: agent.name.clone(),
                scope: Some(ProfileScope::All),
                entry: builtin_entry(agent, profile_kind),
            });
    }

    let agents = files
        .into_values()
        .map(|mut file| {
            if file.entry.status == "available" {
                if let Some(scope) = file.scope {
                    if !scope.includes(profile_kind) {
                        file.entry.status = "hidden".into();
                    }
                }
            }
            file.entry
        })
        .collect();
    Ok(AgentDirectoryResponse {
        profile: profile_name,
        agents,
    })
}

#[tauri::command]
pub(crate) fn agent_directory_open(
    project_dir: Option<String>,
    path: String,
) -> Result<(), String> {
    let cwd = project_dir
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let project_root = kanzei_harness::config::discover_project_root(&cwd).unwrap_or(cwd);
    let candidate = PathBuf::from(path)
        .canonicalize()
        .map_err(|e| format!("Agent 原文不存在: {e}"))?;
    let mut allowed_dirs = vec![project_root.join(".kanzei").join("agents")];
    if let Some(home) = kanzei_harness::kanzei_home() {
        allowed_dirs.push(home.join("agents"));
    }
    let allowed = allowed_dirs
        .into_iter()
        .filter_map(|dir| dir.canonicalize().ok())
        .any(|dir| candidate.starts_with(dir));
    if !allowed || !candidate.is_file() {
        return Err("只允许打开全局或当前项目的 Agent 原文".into());
    }
    std::process::Command::new("cmd")
        .args(["/c", "start", "", &candidate.display().to_string()])
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn scan_agent_files(dir: &Path, source: &str, output: &mut BTreeMap<String, FileAgent>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<_> = read
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .collect();
    paths.sort();
    for path in paths {
        let Some(file) = parse_agent_file(&path, source) else {
            continue;
        };
        output.insert(file.name.clone(), file);
    }
}

fn parse_agent_file(path: &Path, source: &str) -> Option<FileAgent> {
    let text = std::fs::read_to_string(path).ok()?;
    let frontmatter = parse_frontmatter(&text);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("agent");
    let name = frontmatter.get("name").unwrap_or(stem).trim().to_string();
    if name.is_empty() {
        return None;
    }
    let profile_raw = frontmatter.get("profile").unwrap_or("all");
    let mode_raw = frontmatter.get("mode").unwrap_or("primary");
    let model = frontmatter.get("model").unwrap_or("primary").to_string();
    let steps_raw = frontmatter.get("steps").unwrap_or("32");
    let scope = parse_plain::<ProfileScope>(profile_raw);
    let mode = parse_plain::<AgentMode>(mode_raw);
    let steps = steps_raw.parse::<u32>().unwrap_or(DEFAULT_AGENT_STEPS);
    let mut errors = Vec::new();
    if scope.is_none() {
        errors.push(format!("profile `{profile_raw}` 无效"));
    }
    if mode.is_none() {
        errors.push(format!("mode `{mode_raw}` 无效"));
    }
    if steps_raw.parse::<u32>().is_err() {
        errors.push(format!("steps `{steps_raw}` 不是正整数"));
    }
    let error = (!errors.is_empty()).then(|| errors.join("; "));
    Some(FileAgent {
        name: name.clone(),
        scope,
        entry: AgentDirectoryEntry {
            name,
            source: source.into(),
            path: Some(path.display().to_string()),
            profile: scope_label(scope, profile_raw),
            mode: mode_label(mode, mode_raw),
            model,
            steps,
            status: if error.is_some() {
                "configurationError".into()
            } else {
                "available".into()
            },
            system_preview: preview(&frontmatter.body),
            error,
        },
    })
}

fn builtin_entry(agent: &AgentDef, profile: ProfileKind) -> AgentDirectoryEntry {
    AgentDirectoryEntry {
        name: agent.name.clone(),
        source: "builtin".into(),
        path: None,
        profile: scope_label(Some(agent.profile), "all"),
        mode: mode_label(Some(agent.mode), "unknown"),
        model: agent.model.clone(),
        steps: agent.steps,
        status: if agent.profile.includes(profile) {
            "available".into()
        } else {
            "hidden".into()
        },
        system_preview: preview(&agent.system),
        error: None,
    }
}

fn parse_plain<T: serde::de::DeserializeOwned>(value: &str) -> Option<T> {
    serde_json::from_value(serde_json::Value::String(value.to_string())).ok()
}

fn scope_label(scope: Option<ProfileScope>, raw: &str) -> String {
    match scope {
        Some(ProfileScope::Dev) => "dev".into(),
        Some(ProfileScope::Research) => "research".into(),
        Some(ProfileScope::All) => "all".into(),
        None => raw.into(),
    }
}

fn mode_label(mode: Option<AgentMode>, raw: &str) -> String {
    match mode {
        Some(AgentMode::Primary) => "primary".into(),
        Some(AgentMode::Subagent) => "subagent".into(),
        None => raw.into(),
    }
}

fn preview(text: &str) -> String {
    let mut chars = text.chars();
    let value: String = chars.by_ref().take(600).collect();
    if chars.next().is_some() {
        format!("{value}…")
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_agent_frontmatter_is_visible_as_configuration_error() {
        let path = std::env::temp_dir().join(format!(
            "kanzei-agent-directory-{}-invalid.md",
            std::process::id()
        ));
        std::fs::write(
            &path,
            "---\nname: broken\nprofile: nowhere\nmode: mystery\nsteps: nope\n---\n提示词",
        )
        .unwrap();
        let entry = parse_agent_file(&path, "project").unwrap();
        assert_eq!(entry.entry.status, "configurationError");
        assert!(entry.entry.error.unwrap().contains("profile"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn preview_is_bounded() {
        let value = preview(&"x".repeat(601));
        assert_eq!(value.chars().count(), 601);
        assert!(value.ends_with('…'));
    }
}
