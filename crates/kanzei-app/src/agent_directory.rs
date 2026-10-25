//! Settings project the resolved Agent registry and its source diagnostics.
//! Definition parsing and project approval belong to kanzei-harness.

use std::path::{Path, PathBuf};

use serde::Serialize;

use kanzei_harness::agent_sources::{AgentSource, AgentSourceScope};
use kanzei_harness::defs::{effective_agent_steps, AgentMode, ProfileKind};
use kanzei_harness::registry::RegistrationReport;
use kanzei_harness::{AgentDef, KanzeiConfig, ResolveCtx};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentDirectoryEntry {
    pub(crate) name: String,
    pub(crate) source: String,
    pub(crate) path: Option<String>,
    pub(crate) revision: Option<String>,
    pub(crate) approved: Option<bool>,
    pub(crate) profile: String,
    pub(crate) mode: String,
    pub(crate) model: String,
    pub(crate) steps: Option<u32>,
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

fn directory_roots(project_dir: Option<&str>) -> Result<(PathBuf, PathBuf), String> {
    match project_dir {
        Some(path) => {
            let cwd = PathBuf::from(path);
            if !cwd.is_dir() {
                return Err(format!("项目目录不存在: {}", cwd.display()));
            }
            let root = crate::normalized_project_root(&cwd);
            Ok((cwd, root))
        }
        None => {
            let root = crate::general_chat::storage_root()?;
            Ok((root.clone(), root))
        }
    }
}

#[tauri::command]
pub(crate) fn agent_directory_get(
    project_dir: Option<String>,
    profile: Option<String>,
) -> Result<AgentDirectoryResponse, String> {
    let (cwd, project_root) = directory_roots(project_dir.as_deref())?;
    let (config, _) = if crate::general_chat::is_general_root(&project_root) {
        KanzeiConfig::load_global_with_warnings()
    } else {
        KanzeiConfig::load_with_warnings_at_root(&project_root)
    }
    .map_err(|error| format!("读取配置失败: {error}"))?;
    let profile_kind: ProfileKind = match profile {
        Some(profile) => profile.parse(),
        None => config.default_profile(),
    }
    .map_err(|error: String| format!("Agent 目录档位无效: {error}"))?;
    let ctx = ResolveCtx {
        profile: profile_kind,
        cwd,
        project_root,
        config: std::sync::Arc::new(config),
    };
    let harness = crate::run::assembly::build_run_harness(profile_kind, false, None);
    let snapshot = harness
        .resolve(&ctx)
        .map_err(|error| format!("解析 Agent 目录失败: {error}"))?;
    Ok(AgentDirectoryResponse {
        profile: profile_kind.as_str().into(),
        agents: directory_entries(&snapshot),
    })
}

fn apply_source(entry: &mut AgentDirectoryEntry, source: &AgentSource) {
    entry.source = match source.scope {
        AgentSourceScope::Global => "global",
        AgentSourceScope::Project => "project",
    }
    .into();
    entry.path = Some(source.path.display().to_string());
    entry.revision = source.sha256.clone();
    entry.approved = (source.scope == AgentSourceScope::Project).then_some(source.approved);
    entry.error = source.error.clone();
}

fn directory_entries(snapshot: &kanzei_harness::HarnessSnapshot) -> Vec<AgentDirectoryEntry> {
    let registrations = snapshot.registry_report().agents;
    let winners: Vec<_> = registrations
        .iter()
        .filter_map(|item| {
            snapshot
                .agent_sources()
                .iter()
                .enumerate()
                .rfind(|(_, source)| {
                    source.approved
                        && item.winner.path.as_ref() == Some(&source.path)
                        && source
                            .definition()
                            .is_some_and(|agent| agent.name == item.name)
                })
                .map(|(index, _)| (item.name.as_str(), index))
        })
        .collect();
    let mut entries: Vec<_> = snapshot
        .agents()
        .iter()
        .map(|(name, agent)| {
            let mut entry = definition_entry(agent, snapshot.profile());
            entry.registration = registrations.iter().find(|item| item.name == name).cloned();
            if let Some(path) = entry
                .registration
                .as_ref()
                .and_then(|item| item.winner.path.as_ref())
            {
                if let Some((_, index)) = winners.iter().find(|(winner, _)| winner == &name) {
                    let source = &snapshot.agent_sources()[*index];
                    apply_source(&mut entry, source);
                } else {
                    entry.source = "component".into();
                    entry.path = Some(path.display().to_string());
                }
            }
            entry
        })
        .collect();
    for (index, source) in snapshot.agent_sources().iter().enumerate() {
        if winners.iter().any(|(_, winner)| *winner == index) {
            continue;
        }
        let mut entry = match source.definition() {
            Some(agent) => definition_entry(agent, snapshot.profile()),
            None => AgentDirectoryEntry {
                name: source
                    .path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                source: String::new(),
                path: None,
                revision: None,
                approved: None,
                profile: String::new(),
                mode: String::new(),
                model: String::new(),
                steps: None,
                status: String::new(),
                system_preview: String::new(),
                error: None,
                registration: None,
            },
        };
        apply_source(&mut entry, source);
        entry.status = if source.error.is_some() {
            "configurationError"
        } else if !source.approved {
            "untrusted"
        } else {
            "overridden"
        }
        .into();
        entries.push(entry);
    }
    entries.sort_by(|left, right| left.name.cmp(&right.name).then(left.path.cmp(&right.path)));
    entries
}

fn source_file(root: &Path, home: Option<&Path>, requested: &Path) -> Result<PathBuf, String> {
    let candidate = requested
        .canonicalize()
        .map_err(|error| format!("Agent 原文不存在: {error}"))?;
    if !candidate.is_file()
        || !candidate
            .extension()
            .is_some_and(|extension| extension == "md")
    {
        return Err("只允许打开 Agent Markdown 原文".into());
    }
    let mut directories = Vec::new();
    if !crate::general_chat::is_general_root(root) {
        directories.push(root.join(".kanzei/agents"));
    }
    if let Some(home) = home {
        directories.push(home.join("agents"));
    }
    for directory in directories {
        let directory = match directory.canonicalize() {
            Ok(directory) => directory,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("{}: {error}", directory.display())),
        };
        if candidate.parent() != Some(directory.as_path()) {
            continue;
        }
        let resource = kanzei_harness::permission::FileResource::resolve(&directory, &candidate)
            .map_err(|error| error.to_string())?;
        if resource.sensitivity().is_some() {
            return Err("Agent 原文指向受保护文件".into());
        }
        return Ok(candidate);
    }
    Err("只允许打开全局或当前项目的 Agent 原文".into())
}

#[tauri::command]
pub(crate) fn agent_directory_open(
    project_dir: Option<String>,
    path: String,
) -> Result<(), String> {
    let (_, root) = directory_roots(project_dir.as_deref())?;
    let candidate = source_file(
        &root,
        kanzei_harness::kanzei_home().as_deref(),
        Path::new(&path),
    )?;
    // Pass the source as one editor argument, without shell interpretation.
    std::process::Command::new("notepad.exe")
        .arg(candidate)
        .spawn()
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn definition_entry(agent: &AgentDef, profile: ProfileKind) -> AgentDirectoryEntry {
    AgentDirectoryEntry {
        name: agent.name.clone(),
        source: "builtin".into(),
        path: None,
        revision: None,
        approved: None,
        profile: agent.profile.as_str().into(),
        mode: match agent.mode {
            AgentMode::Primary => "primary",
            AgentMode::Subagent => "subagent",
        }
        .into(),
        model: agent.model.clone(),
        steps: Some(effective_agent_steps(agent.steps, agent.mode)),
        status: if agent.profile.includes(profile) {
            "available"
        } else {
            "hidden"
        }
        .into(),
        system_preview: preview(&agent.system),
        error: None,
        registration: None,
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
    use kanzei_harness::{Component, Harness, HarnessDraft, ProfileScope};

    #[test]
    fn preview_is_bounded() {
        let value = preview(&"中".repeat(601));
        assert_eq!(value.chars().count(), 601);
        assert!(value.ends_with('…'));
    }

    #[test]
    fn directory_projects_registered_sources_unapproved_overrides_and_errors() {
        kanzei_base::test_support::isolated_test(
            module_path!(),
            "directory_projects_registered_sources_unapproved_overrides_and_errors",
            || {
                struct Definitions;
                impl Component for Definitions {
                    fn contribute(
                        &self,
                        draft: &mut HarnessDraft,
                        _: &ResolveCtx,
                    ) -> anyhow::Result<()> {
                        draft.agents.insert(
                            "custom",
                            AgentDef {
                                name: "custom".into(),
                                profile: ProfileScope::DevAuto,
                                mode: AgentMode::Primary,
                                model: "primary".into(),
                                steps: 0,
                                system: "builtin".into(),
                            },
                        );
                        Ok(())
                    }
                }
                let directory = kanzei_base::test_support::TestDir::new("kz-agent-directory-");
                let root = directory.join("project");
                let home = directory.join("home");
                std::fs::create_dir_all(root.join(".kanzei/agents")).unwrap();
                std::fs::create_dir_all(home.join("agents")).unwrap();
                std::env::set_var("KANZEI_HOME", &home);
                std::fs::write(
                    home.join("agents/custom.md"),
                    "---\nname: custom\n---\nglobal",
                )
                .unwrap();
                std::fs::write(
                    root.join(".kanzei/agents/custom.md"),
                    "---\nname: custom\nmode: subagent\nsteps: 7\n---\nproject",
                )
                .unwrap();
                std::fs::write(
                    root.join(".kanzei/agents/broken.md"),
                    "---\nmode: invalid\n---\ninvalid replacement",
                )
                .unwrap();
                let mut harness = Harness::default();
                harness
                    .add(Definitions)
                    .add(kanzei_harness::MarkdownComponent);
                let ctx = ResolveCtx {
                    profile: ProfileKind::DevAuto,
                    cwd: root.clone(),
                    project_root: root.clone(),
                    config: std::sync::Arc::new(KanzeiConfig::default()),
                };
                let snapshot = harness.resolve(&ctx).unwrap();
                let entries = directory_entries(&snapshot);
                assert_eq!(
                    entries
                        .iter()
                        .find(|entry| entry.status == "available")
                        .unwrap()
                        .system_preview,
                    "global"
                );
                assert!(entries.iter().any(|entry| entry.status == "untrusted"
                    && entry.system_preview == "project"
                    && entry.approved == Some(false)));
                assert!(entries
                    .iter()
                    .any(|entry| entry.status == "configurationError"
                        && entry.error.as_ref().unwrap().contains("invalid")));
                let sources = kanzei_harness::project_trust::status(&root).unwrap();
                kanzei_harness::project_trust::approve(&root, &sources.fingerprint).unwrap();
                let snapshot = harness.resolve(&ctx).unwrap();
                let entries = directory_entries(&snapshot);
                let effective = entries
                    .iter()
                    .find(|entry| entry.status == "available")
                    .unwrap();
                assert_eq!(effective.source, "project");
                assert_eq!(effective.approved, Some(true));
                assert_eq!(effective.steps, Some(0));
                assert_eq!(effective.registration.as_ref().unwrap().replaced.len(), 2);
                assert!(entries
                    .iter()
                    .any(|entry| entry.status == "overridden" && entry.system_preview == "global"));
                std::fs::write(
                    root.join(".kanzei/agents/custom.md"),
                    "Changed after resolution",
                )
                .unwrap();
                assert_eq!(
                    directory_entries(&snapshot)
                        .iter()
                        .find(|entry| entry.status == "available")
                        .unwrap()
                        .system_preview,
                    "project"
                );
                assert_eq!(effective.revision.as_ref().unwrap().len(), 64);
                let diagnostic = serde_json::to_value(snapshot.registry_report()).unwrap();
                assert!(diagnostic["agent_sources"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|source| source.get("definition").is_none()));
            },
        );
    }

    #[test]
    fn source_open_accepts_only_top_level_markdown_in_the_selected_scope() {
        let directory = kanzei_base::test_support::TestDir::new("kz-agent-open-");
        let root = directory.join("project");
        let agents = root.join(".kanzei/agents");
        std::fs::create_dir_all(agents.join("nested")).unwrap();
        let allowed = agents.join("agent & source.md");
        std::fs::write(&allowed, "Agent").unwrap();
        std::fs::write(agents.join("script.cmd"), "Command").unwrap();
        std::fs::write(agents.join("nested/other.md"), "Other").unwrap();
        assert!(source_file(&root, None, &allowed).is_ok());
        assert!(source_file(&root, None, &agents.join("script.cmd")).is_err());
        assert!(source_file(&root, None, &agents.join("nested/other.md")).is_err());
        assert!(source_file(&directory.join("other-project"), None, &allowed).is_err());
    }
}
