//! Agent 目录只读入口(R-305 B1)。
//!
//! 运行时 Agent 仍由 Harness 解析；本模块只把同一套全局/项目 Markdown 来源与
//! 内建注册表投影给设置页，并把配置错误显式呈现，不提供编辑或启停旁路。

use std::path::{Path, PathBuf};

use serde::Serialize;

use kanzei_harness::defs::{effective_agent_steps, AgentMode, ProfileKind, ProfileScope};
use kanzei_harness::markdown::parse_frontmatter;
use kanzei_harness::registry::RegistrationReport;
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
    pub(crate) registration: Option<RegistrationReport>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentDirectoryResponse {
    pub(crate) profile: String,
    pub(crate) agents: Vec<AgentDirectoryEntry>,
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
    let project_root = if project_dir.is_some() {
        crate::normalized_project_root(&cwd)
    } else {
        kanzei_harness::config::discover_project_root(&cwd).unwrap_or(cwd)
    };
    let (config, _) = KanzeiConfig::load_with_warnings_at_root(&project_root)
        .map_err(|e| format!("读取配置失败: {e}"))?;
    let profile_name = profile
        .or_else(|| config.profile.default.clone())
        .unwrap_or_else(|| "dev".into());
    let profile_kind: ProfileKind = profile_name
        .parse()
        .map_err(|e: String| format!("Agent 目录档位无效: {e}"))?;

    let mut files = Vec::new();
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
    let harness = crate::run::assembly::build_run_harness(profile_kind, false, None);
    let snapshot = harness
        .resolve(&ctx)
        .map_err(|e| format!("解析内建 Agent 失败: {e}"))?;
    let agents = directory_entries(&snapshot, files, &project_root);
    Ok(AgentDirectoryResponse {
        profile: profile_name,
        agents,
    })
}

fn directory_entries(
    snapshot: &kanzei_harness::HarnessSnapshot,
    files: Vec<AgentDirectoryEntry>,
    project_root: &Path,
) -> Vec<AgentDirectoryEntry> {
    let registrations = snapshot.registry_report().agents;
    let project_agents = project_root.join(".kanzei/agents");
    let mut entries: Vec<_> = snapshot
        .agents()
        .iter()
        .map(|(name, agent)| {
            let mut entry = builtin_entry(agent, snapshot.profile());
            entry.registration = registrations.iter().find(|item| item.name == name).cloned();
            if let Some(path) = entry
                .registration
                .as_ref()
                .and_then(|item| item.winner.path.as_ref())
            {
                entry.source = if path.starts_with(&project_agents) {
                    "project"
                } else {
                    "global"
                }
                .into();
                entry.path = Some(path.display().to_string());
            }
            entry
        })
        .collect();
    // 无效文件未进入运行注册表,单独显示诊断,不能冒充同名实际胜者。
    entries.extend(files.into_iter().filter(|entry| entry.error.is_some()));
    entries.sort_by(|left, right| left.name.cmp(&right.name).then(left.path.cmp(&right.path)));
    entries
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
    let project_root = if project_dir.is_some() {
        crate::normalized_project_root(&cwd)
    } else {
        kanzei_harness::config::discover_project_root(&cwd).unwrap_or(cwd)
    };
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

fn scan_agent_files(dir: &Path, source: &str, output: &mut Vec<AgentDirectoryEntry>) {
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
        output.push(file);
    }
}

fn parse_agent_file(path: &Path, source: &str) -> Option<AgentDirectoryEntry> {
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
    let steps_raw = frontmatter.get("steps").unwrap_or("0");
    let scope = parse_plain::<ProfileScope>(profile_raw);
    let mode = parse_plain::<AgentMode>(mode_raw);
    let steps = effective_agent_steps(
        steps_raw.parse::<u32>().unwrap_or_default(),
        mode.unwrap_or_default(),
    );
    let mut errors = Vec::new();
    if scope.is_none() {
        errors.push(format!("profile `{profile_raw}` 无效"));
    }
    if mode.is_none() {
        errors.push(format!("mode `{mode_raw}` 无效"));
    }
    if steps_raw.parse::<u32>().is_err() {
        errors.push(format!("steps `{steps_raw}` 不是非负整数"));
    }
    let error = (!errors.is_empty()).then(|| errors.join("; "));
    Some(AgentDirectoryEntry {
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
        registration: None,
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
        steps: effective_agent_steps(agent.steps, agent.mode),
        status: if agent.profile.includes(profile) {
            "available".into()
        } else {
            "hidden".into()
        },
        system_preview: preview(&agent.system),
        error: None,
        registration: None,
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
        assert_eq!(entry.status, "configurationError");
        assert!(entry.error.unwrap().contains("profile"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn preview_is_bounded() {
        let value = preview(&"x".repeat(601));
        assert_eq!(value.chars().count(), 601);
        assert!(value.ends_with('…'));
    }

    #[test]
    fn directory_uses_registered_winner_and_keeps_invalid_override_visible() {
        struct Definitions;
        impl kanzei_harness::Component for Definitions {
            fn contribute(
                &self,
                draft: &mut kanzei_harness::HarnessDraft,
                ctx: &ResolveCtx,
            ) -> anyhow::Result<()> {
                let definition = |system: &str| AgentDef {
                    name: "custom".into(),
                    profile: ProfileScope::Dev,
                    mode: AgentMode::Primary,
                    model: "primary".into(),
                    steps: 0,
                    system: system.into(),
                };
                draft.agents.insert("custom", definition("builtin"));
                draft.agents.insert_at(
                    "custom",
                    definition("global"),
                    ctx.project_root.join("global/agents/custom.md"),
                );
                draft.agents.insert_at(
                    "custom",
                    definition("project"),
                    ctx.project_root.join(".kanzei/agents/custom.md"),
                );
                Ok(())
            }
        }
        let root = PathBuf::from("C:/agent-directory-winner-fixture");
        let mut harness = kanzei_harness::Harness::default();
        harness.add(Definitions);
        let snapshot = harness
            .resolve(&ResolveCtx {
                profile: ProfileKind::Dev,
                cwd: root.clone(),
                project_root: root.clone(),
                config: std::sync::Arc::new(KanzeiConfig::default()),
            })
            .unwrap();
        let mut invalid = builtin_entry(
            snapshot.select_primary_agent(Some("custom")).unwrap(),
            ProfileKind::Dev,
        );
        invalid.status = "configurationError".into();
        invalid.error = Some("mode `invalid` 无效".into());
        invalid.path = Some(
            root.join(".kanzei/agents/z-invalid.md")
                .display()
                .to_string(),
        );
        invalid.system_preview = "invalid replacement".into();
        let entries = directory_entries(&snapshot, vec![invalid], &root);
        assert_eq!(entries.len(), 2);
        let effective = entries
            .iter()
            .find(|entry| entry.status == "available")
            .unwrap();
        assert_eq!(effective.source, "project");
        assert_eq!(effective.system_preview, "project");
        assert_eq!(
            effective.path,
            Some(root.join(".kanzei/agents/custom.md").display().to_string())
        );
        assert_eq!(effective.registration.as_ref().unwrap().replaced.len(), 2);
        assert!(entries
            .iter()
            .any(|entry| entry.status == "configurationError"
                && entry.system_preview == "invalid replacement"));
    }

    #[test]
    fn directory_shows_effective_budget_for_each_role() {
        let path = std::env::temp_dir().join(format!(
            "kanzei-agent-directory-{}-budget.md",
            std::process::id()
        ));
        for (mode, explicit, expected) in [
            ("primary", "", 0),
            ("subagent", "", 0),
            ("primary", "steps: 7\n", 7),
            ("subagent", "steps: 7\n", 0),
        ] {
            std::fs::write(
                &path,
                format!("---\nname: budget\nmode: {mode}\n{explicit}---\nPrompt"),
            )
            .unwrap();
            let file = parse_agent_file(&path, "project").unwrap();
            assert_eq!(file.steps, expected);
            assert_eq!(file.status, "available");
        }
        std::fs::remove_file(path).unwrap();
    }
}
