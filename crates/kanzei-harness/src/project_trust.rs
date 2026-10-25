//! User-owned, content-bound approval of executable project configuration.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(crate) const MAX_SOURCE_BYTES: u64 = 1024 * 1024;

#[derive(Serialize)]
pub struct SourceStatus {
    pub path: String,
    pub sha256: String,
    pub approved: bool,
    #[serde(skip)]
    pub(crate) text: String,
}

#[derive(Serialize)]
pub struct ProjectSources {
    pub fingerprint: String,
    pub files: Vec<SourceStatus>,
}

#[derive(Default, Deserialize, Serialize)]
struct Approvals {
    #[serde(default)]
    files: BTreeMap<String, String>,
    #[serde(default)]
    permissions: BTreeMap<String, Vec<crate::Rule>>,
}

impl ProjectSources {
    pub fn load(root: &Path, home: Option<&Path>) -> anyhow::Result<Self> {
        let dir = match cap_std::fs::Dir::open_ambient_dir(root, cap_std::ambient_authority()) {
            Ok(dir) => dir,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self {
                    fingerprint: digest(&[]),
                    files: Vec::new(),
                })
            }
            Err(error) => return Err(error.into()),
        };
        let mut names = Vec::new();
        if dir.try_exists(".kanzei/kanzei.toml")? {
            names.push(".kanzei/kanzei.toml".to_string());
        }
        if dir.try_exists(".kanzei/agents")? {
            for entry in dir.read_dir(".kanzei/agents")? {
                let entry = entry?;
                if Path::new(&entry.file_name())
                    .extension()
                    .is_some_and(|extension| extension == "md")
                {
                    names.push(format!(
                        ".kanzei/agents/{}",
                        entry.file_name().to_string_lossy()
                    ));
                }
            }
        }
        names.sort();
        let approvals = read_approvals(home)?;
        let mut files = Vec::new();
        let mut manifest = Sha256::new();
        for name in names {
            if name.starts_with(".kanzei/agents/") {
                let path = std::path::absolute(root.join(&name))?;
                let resource = crate::permission::FileResource::resolve(root, &path)?;
                anyhow::ensure!(
                    resource.sensitivity().is_none(),
                    "Agent source {name} resolves to a protected file"
                );
            }
            let mut bytes = Vec::new();
            dir.open(&name)?
                .take(MAX_SOURCE_BYTES + 1)
                .read_to_end(&mut bytes)?;
            anyhow::ensure!(
                bytes.len() as u64 <= MAX_SOURCE_BYTES,
                "project source {name} exceeds the 1 MiB configuration/agent limit"
            );
            let sha256 = digest(&bytes);
            manifest.update(name.as_bytes());
            manifest.update([0]);
            manifest.update(sha256.as_bytes());
            let approved = approvals.files.get(&source_key(root, &name)?) == Some(&sha256);
            files.push(SourceStatus {
                path: name,
                sha256,
                approved,
                text: String::from_utf8(bytes)?,
            });
        }
        Ok(Self {
            fingerprint: format!("{:x}", manifest.finalize()),
            files,
        })
    }

    pub fn approved_config(&self) -> Option<&str> {
        self.files
            .iter()
            .find(|source| source.path == ".kanzei/kanzei.toml" && source.approved)
            .map(|source| source.text.as_str())
    }
}

pub fn status(root: &Path) -> anyhow::Result<ProjectSources> {
    ProjectSources::load(root, crate::kanzei_home().as_deref())
}

pub fn approve(root: &Path, fingerprint: &str) -> anyhow::Result<()> {
    let home = crate::kanzei_home()
        .ok_or_else(|| anyhow::anyhow!("user configuration home is unavailable"))?;
    approve_at(root, &home, fingerprint)
}

pub fn approve_at(root: &Path, home: &Path, fingerprint: &str) -> anyhow::Result<()> {
    let sources = ProjectSources::load(root, Some(home))?;
    anyhow::ensure!(sources.fingerprint == fingerprint, "project sources changed; run kz trust status again and review the new files before approving");
    update_approvals(home, |approvals| {
        for source in sources.files {
            approvals
                .files
                .insert(source_key(root, &source.path)?, source.sha256);
        }
        Ok(())
    })
}

pub fn remember_permission(root: &Path, action: &str, resource: &str) -> anyhow::Result<PathBuf> {
    let home = crate::kanzei_home()
        .ok_or_else(|| anyhow::anyhow!("user configuration home is unavailable"))?;
    remember_permission_at(root, &home, action, resource)
}

pub fn remember_permission_at(
    root: &Path,
    home: &Path,
    action: &str,
    resource: &str,
) -> anyhow::Result<PathBuf> {
    let key = root_key(root)?;
    update_approvals(home, |approvals| {
        let rules = approvals.permissions.entry(key).or_default();
        if !rules.iter().any(|rule| {
            rule.action == action
                && rule.resource == resource
                && rule.effect == crate::Effect::Allow
        }) {
            rules.push(crate::rule(action, resource, crate::Effect::Allow));
        }
        Ok(())
    })?;
    Ok(home.join("project-trust.json"))
}

pub fn remembered_permissions(
    root: &Path,
    home: Option<&Path>,
) -> anyhow::Result<Vec<crate::Rule>> {
    Ok(read_approvals(home)?
        .permissions
        .remove(&root_key(root)?)
        .unwrap_or_default())
}

pub fn forget_permission(root: &Path, index: usize, expected: &crate::Rule) -> anyhow::Result<()> {
    let home = crate::kanzei_home()
        .ok_or_else(|| anyhow::anyhow!("user configuration home is unavailable"))?;
    let key = root_key(root)?;
    update_approvals(&home, |approvals| {
        let rules = approvals
            .permissions
            .get_mut(&key)
            .ok_or_else(|| anyhow::anyhow!("权限规则不存在或已被删除"))?;
        let rule = rules
            .get(index)
            .ok_or_else(|| anyhow::anyhow!("权限规则不存在或已被删除"))?;
        anyhow::ensure!(
            rule.action == expected.action
                && rule.resource == expected.resource
                && rule.effect == expected.effect,
            "权限规则已变化，请刷新后重试"
        );
        rules.remove(index);
        Ok(())
    })
}

pub fn revoke(root: &Path) -> anyhow::Result<()> {
    let home = crate::kanzei_home()
        .ok_or_else(|| anyhow::anyhow!("user configuration home is unavailable"))?;
    let prefix = format!("{}/", root_key(root)?);
    update_approvals(&home, |approvals| {
        approvals.files.retain(|key, _| !key.starts_with(&prefix));
        approvals.permissions.remove(prefix.trim_end_matches('/'));
        Ok(())
    })
}

fn update_approvals(
    home: &Path,
    update: impl FnOnce(&mut Approvals) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(home)?;
    let path = home.join("project-trust.json");
    let _lock = kanzei_base::atomic_file::lock_exclusive(&path)?;
    let mut approvals = read_approvals(Some(home))?;
    update(&mut approvals)?;
    kanzei_base::atomic_file::write_atomic_bytes(&path, &serde_json::to_vec_pretty(&approvals)?)?;
    Ok(())
}

fn read_approvals(home: Option<&Path>) -> anyhow::Result<Approvals> {
    let Some(home) = home else {
        return Ok(Approvals::default());
    };
    match std::fs::read(home.join("project-trust.json")) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Approvals::default()),
        Err(error) => Err(error.into()),
    }
}

fn root_key(root: &Path) -> anyhow::Result<String> {
    let root = kanzei_base::path_form::canonical(root)?;
    let raw = root.to_string_lossy();
    let key = crate::permission::normalize_resource(&kanzei_base::path_form::strip_verbatim(&raw));
    Ok(digest(key.as_bytes()))
}

fn source_key(root: &Path, name: &str) -> anyhow::Result<String> {
    Ok(format!("{}/{}", root_key(root)?, name))
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approvals_bind_root_and_source_content_and_do_not_approve_future_agents() {
        let root = std::env::temp_dir().join(format!(
            "kz-trust-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let project = root.join("project");
        let home = root.join("user-home");
        std::fs::create_dir_all(project.join(".kanzei/agents")).unwrap();
        std::fs::write(
            project.join(".kanzei/kanzei.toml"),
            "[permissions]\nrules = [{action='bash',resource='*',effect='allow'}]\n",
        )
        .unwrap();
        let before = ProjectSources::load(&project, Some(&home)).unwrap();
        assert!(before.approved_config().is_none());
        approve_at(&project, &home, &before.fingerprint).unwrap();
        assert!(ProjectSources::load(&project, Some(&home))
            .unwrap()
            .approved_config()
            .is_some());
        std::fs::write(
            project.join(".kanzei/agents/dev.md"),
            "malicious replacement",
        )
        .unwrap();
        let added = ProjectSources::load(&project, Some(&home)).unwrap();
        assert!(added.approved_config().is_some());
        assert!(
            !added
                .files
                .iter()
                .find(|source| source.path.ends_with("dev.md"))
                .unwrap()
                .approved
        );
        std::fs::write(
            project.join(".kanzei/kanzei.toml"),
            "proxy = 'http://attacker.test:80'",
        )
        .unwrap();
        assert!(ProjectSources::load(&project, Some(&home))
            .unwrap()
            .approved_config()
            .is_none());
        assert!(approve_at(&project, &home, &before.fingerprint).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
