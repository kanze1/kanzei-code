//! One read/parse owner for custom Agent registration and source diagnostics.
//! Project definitions use the exact content snapshot covered by source approval.

use std::{
    io::Read,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{AgentDef, AgentMode, HarnessDraft, ProfileScope};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentSourceScope {
    Global,
    Project,
}

#[derive(Clone, Serialize)]
pub struct AgentSource {
    pub scope: AgentSourceScope,
    pub path: PathBuf,
    pub sha256: Option<String>,
    /// Global definitions are user-owned. Project definitions require content-bound approval.
    pub approved: bool,
    pub error: Option<String>,
    #[serde(skip)]
    definition: Option<AgentDef>,
}

impl AgentSource {
    pub fn definition(&self) -> Option<&AgentDef> {
        self.definition.as_ref()
    }

    fn parsed(
        scope: AgentSourceScope,
        path: PathBuf,
        sha256: Option<String>,
        approved: bool,
        definition: Result<AgentDef, String>,
    ) -> Self {
        let (definition, error) = match definition {
            Ok(definition) => (Some(definition), None),
            Err(error) => (None, Some(error)),
        };
        Self {
            scope,
            path,
            sha256,
            approved,
            definition,
            error,
        }
    }
}

impl std::fmt::Debug for AgentSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentSource")
            .field("scope", &self.scope)
            .field("path", &self.path)
            .field("sha256", &self.sha256)
            .field("approved", &self.approved)
            .field("error", &self.error)
            .finish()
    }
}

/// Missing directories are empty; enumeration failures remain errors. Individual
/// file failures remain visible in the catalog and never register an override.
pub(crate) fn load_at(
    root: Option<&Path>,
    home: Option<&Path>,
) -> anyhow::Result<Vec<AgentSource>> {
    let mut sources = match home {
        Some(home) => load_global(&home.join("agents"))?,
        None => Vec::new(),
    };
    if let Some(root) = root {
        let project = crate::project_trust::ProjectSources::load(root, home)?;
        sources.extend(
            project
                .files
                .into_iter()
                .filter(|source| source.path.starts_with(".kanzei/agents/"))
                .map(|source| {
                    let path = root.join(source.path);
                    let definition = parse(&path, &source.text);
                    AgentSource::parsed(
                        AgentSourceScope::Project,
                        path,
                        Some(source.sha256),
                        source.approved,
                        definition,
                    )
                }),
        );
    }
    Ok(sources)
}

fn load_global(directory: &Path) -> anyhow::Result<Vec<AgentSource>> {
    let read = match std::fs::read_dir(directory) {
        Ok(read) => read,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(anyhow::anyhow!("{}: {error}", directory.display())),
    };
    let mut paths = Vec::new();
    for entry in read {
        let path = entry?.path();
        if path.extension().is_some_and(|extension| extension == "md") {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths
        .into_iter()
        .map(|path| {
            let read = (|| -> anyhow::Result<Vec<u8>> {
                let absolute = std::path::absolute(&path)?;
                let file = crate::permission::ScopedFile::resolve(directory, directory, &absolute)?;
                let resource = crate::permission::FileResource::resolve(directory, &absolute)?;
                anyhow::ensure!(
                    resource.sensitivity().is_none(),
                    "Agent source resolves to a protected file"
                );
                let mut bytes = Vec::new();
                file.open()?
                    .take(crate::project_trust::MAX_SOURCE_BYTES + 1)
                    .read_to_end(&mut bytes)?;
                anyhow::ensure!(
                    bytes.len() as u64 <= crate::project_trust::MAX_SOURCE_BYTES,
                    "Agent source exceeds the 1 MiB limit"
                );
                Ok(bytes)
            })();
            match read {
                Ok(bytes) => {
                    let revision = format!("{:x}", Sha256::digest(&bytes));
                    let definition = String::from_utf8(bytes)
                        .map_err(|error| error.to_string())
                        .and_then(|text| parse(&path, &text));
                    AgentSource::parsed(
                        AgentSourceScope::Global,
                        path,
                        Some(revision),
                        true,
                        definition,
                    )
                }
                Err(error) => AgentSource::parsed(
                    AgentSourceScope::Global,
                    path,
                    None,
                    true,
                    Err(error.to_string()),
                ),
            }
        })
        .collect())
}

pub(crate) fn register(sources: Vec<AgentSource>, draft: &mut HarnessDraft) {
    for source in &sources {
        if let Some(error) = &source.error {
            tracing::warn!(path = %source.path.display(), %error, "invalid Agent source; not registered");
        }
        if let Some(agent) = source.definition().filter(|_| source.approved) {
            draft
                .agents
                .insert_at(agent.name.clone(), agent.clone(), source.path.clone());
        }
    }
    draft.agent_sources.extend(sources);
}

fn parse(path: &Path, text: &str) -> Result<AgentDef, String> {
    #[derive(Default, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Metadata {
        name: Option<String>,
        #[serde(default)]
        profile: ProfileScope,
        #[serde(default)]
        mode: AgentMode,
        model: Option<String>,
        #[serde(default)]
        steps: u32,
    }
    let text = text.trim_start_matches('\u{feff}');
    let first_line = text.split_inclusive('\n').next().unwrap_or("");
    let (metadata, body) = if first_line.trim() == "---" {
        let mut offset = first_line.len();
        let mut closing = None;
        for line in text[offset..].split_inclusive('\n') {
            if line.trim() == "---" {
                closing = Some((offset, offset + line.len()));
                break;
            }
            offset += line.len();
        }
        let (yaml_end, body_start) = closing.ok_or("Agent frontmatter 缺少闭合 ---")?;
        let yaml = &text[first_line.len()..yaml_end];
        let metadata: Metadata = if yaml.trim().is_empty() {
            Metadata::default()
        } else {
            serde_yaml_ng::from_str(yaml).map_err(|error| format!("Agent frontmatter: {error}"))?
        };
        (metadata, &text[body_start..])
    } else {
        (Metadata::default(), text)
    };
    let name = metadata.name.unwrap_or_else(|| {
        path.file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    });
    let name = name.trim().to_string();
    let model = metadata.model.unwrap_or_else(|| "primary".into());
    let model = model.trim().to_string();
    if name.is_empty() {
        return Err("Agent name 不能为空".into());
    }
    if model.is_empty() {
        return Err("Agent model 不能为空".into());
    }
    Ok(AgentDef {
        name,
        profile: metadata.profile,
        mode: metadata.mode,
        model,
        steps: metadata.steps,
        system: body.trim().into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_defaults_multiline_and_errors_share_one_parser() {
        let path = Path::new("custom.md");
        let bare = parse(path, "Prompt").unwrap();
        assert_eq!(bare.name, "custom");
        assert_eq!(bare.mode, AgentMode::Primary);
        assert_eq!(bare.profile, ProfileScope::All);
        assert_eq!(bare.steps, 0);
        let definition = parse(path, "\u{feff}---\r\nname: custom\r\nprofile: dev-pair\r\nmodel: 'primary'\r\nmode: subagent\r\nsteps: 7\r\n---\r\n多行\r\n正文").unwrap();
        assert_eq!(definition.profile, ProfileScope::DevPair);
        assert_eq!(definition.model, "primary");
        assert_eq!(definition.system, "多行\r\n正文");
        for field in [
            "name: ''",
            "model: ''",
            "profile: unknown",
            "mode: typo",
            "steps: -1",
            "steps: nope",
            "typo: value",
            "mode: primary\nmode: subagent",
        ] {
            assert!(
                parse(path, &format!("---\n{field}\n---\nPrompt")).is_err(),
                "{field}"
            );
        }
        assert!(parse(path, "---\nname: custom").is_err());
    }

    #[test]
    fn project_overrides_use_approved_snapshot_and_report_all_sources() {
        let directory = kanzei_base::test_support::TestDir::new("kz-agent-sources-");
        let root = directory.join("project");
        let home = directory.join("home");
        std::fs::create_dir_all(root.join(".kanzei/agents")).unwrap();
        std::fs::create_dir_all(home.join("agents")).unwrap();
        std::fs::write(home.join("agents/custom.md"), "Global prompt").unwrap();
        let project_path = root.join(".kanzei/agents/custom.md");
        std::fs::write(&project_path, "Project prompt").unwrap();
        let mut draft = HarnessDraft::default();
        register(load_at(Some(&root), Some(&home)).unwrap(), &mut draft);
        assert_eq!(draft.agents.get("custom").unwrap().system, "Global prompt");
        assert_eq!(draft.agent_sources.len(), 2);
        assert!(!draft.agent_sources[1].approved);
        let status = crate::project_trust::ProjectSources::load(&root, Some(&home)).unwrap();
        crate::project_trust::approve_at(&root, &home, &status.fingerprint).unwrap();
        let sources = load_at(Some(&root), Some(&home)).unwrap();
        std::fs::write(&project_path, "Changed after snapshot").unwrap();
        let mut draft = HarnessDraft::default();
        register(sources, &mut draft);
        assert_eq!(draft.agents.get("custom").unwrap().system, "Project prompt");
        assert_eq!(draft.agents.report()[0].replaced.len(), 1);
        let changed = load_at(Some(&root), Some(&home)).unwrap();
        assert!(!changed[1].approved);
        assert_ne!(changed[1].sha256, draft.agent_sources[1].sha256);
        assert!(!serde_json::to_string(&draft.agent_sources)
            .unwrap()
            .contains("Project prompt"));
        std::fs::write(home.join("agents/broken.md"), [0xff, 0xfe]).unwrap();
        assert!(load_at(Some(&root), Some(&home))
            .unwrap()
            .iter()
            .any(|source| source.error.is_some()));
        assert!(load_at(None, Some(&home))
            .unwrap()
            .iter()
            .all(|source| source.scope == AgentSourceScope::Global));
    }
}
