//! Execution identity shared by requests, work claims and verification receipts.
use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{ensure, Context};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceFile {
    pub path: String,
    pub sha256: String,
    pub executable: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceIdentity {
    pub head: Option<String>,
    pub fingerprint: String,
    pub file_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PythonEnvironment {
    pub root: PathBuf,
    pub executable: PathBuf,
    pub configuration_sha256: String,
    pub executable_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BinaryIdentity {
    pub path: PathBuf,
    pub sha256: String,
    pub version: String,
}

fn binary_identity() -> anyhow::Result<BinaryIdentity> {
    static CURRENT: OnceLock<BinaryIdentity> = OnceLock::new();
    if let Some(identity) = CURRENT.get() {
        return Ok(identity.clone());
    }
    let path = kanzei_base::path_form::canonical(&std::env::current_exe()?)?;
    let mut file = std::fs::File::open(&path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    let identity = BinaryIdentity {
        path,
        sha256: format!("{:x}", digest.finalize()),
        version: env!("CARGO_PKG_VERSION").into(),
    };
    Ok(CURRENT.get_or_init(|| identity).clone())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionIdentity {
    #[serde(default)]
    pub harness: Option<BinaryIdentity>,
    pub cwd: PathBuf,
    pub project_root: PathBuf,
    pub source: Option<SourceIdentity>,
    pub python: Option<PythonEnvironment>,
}

fn digest(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}

fn git(root: &Path, args: &[&str]) -> anyhow::Result<std::process::Output> {
    let mut command = std::process::Command::new("git");
    command.current_dir(root).args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    Ok(command.output()?)
}

pub fn executable(path: &Path) -> anyhow::Result<bool> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Ok(std::fs::metadata(path)?.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(false)
    }
}

/// All Git-visible inputs, independent of language and of the commit gate's scope.
/// Managed records are evidence outputs, so they do not change the source identity.
pub fn source_snapshot(root: &Path) -> anyhow::Result<Option<(SourceIdentity, Vec<SourceFile>)>> {
    let root = kanzei_base::path_form::canonical(root)?;
    if !root.ancestors().any(|path| path.join(".git").exists()) {
        return Ok(None);
    }
    let output = git(
        &root,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    )?;
    ensure!(
        output.status.success(),
        "cannot enumerate execution source: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut paths = BTreeSet::new();
    for name in String::from_utf8(output.stdout)?
        .split('\0')
        .filter(|p| !p.is_empty())
    {
        let relative = Path::new(name);
        ensure!(
            relative
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_))),
            "invalid source path: {name}"
        );
        if name == ".kanzei" || name.starts_with(".kanzei/") {
            continue;
        }
        match std::fs::symlink_metadata(root.join(relative)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
            Ok(metadata) => ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink(),
                "source must be a regular file: {name}"
            ),
        }
        ensure!(
            kanzei_base::path_form::canonical(&root.join(relative))?.starts_with(&root),
            "source path escapes working tree: {name}"
        );
        paths.insert(name.to_string());
    }
    ensure!(paths.len() <= 30_000, "snapshot has more than 30000 files");
    let mut total = 0u64;
    let mut files = Vec::new();
    for name in paths {
        let path = root.join(&name);
        let bytes = std::fs::read(&path)?;
        total += bytes.len() as u64;
        ensure!(
            total <= 512 * 1024 * 1024,
            "snapshot exceeds 512 MiB; narrow the source tree"
        );
        files.push(SourceFile {
            path: name,
            sha256: digest(bytes),
            executable: executable(&path)?,
        });
    }
    let head = git(&root, &["rev-parse", "--verify", "--quiet", "HEAD"])?;
    let head = match head.status.code() {
        Some(0) => Some(String::from_utf8(head.stdout)?.trim().to_string()),
        Some(1) => None, // A Git repository with no first commit.
        _ => anyhow::bail!(
            "cannot resolve execution HEAD: {}",
            String::from_utf8_lossy(&head.stderr)
        ),
    };
    let identity = SourceIdentity {
        head,
        fingerprint: digest(serde_json::to_vec(&files)?),
        file_count: files.len(),
    };
    Ok(Some((identity, files)))
}

impl ExecutionIdentity {
    pub fn capture(cwd: &Path, project_root: &Path) -> anyhow::Result<Self> {
        let cwd = kanzei_base::path_form::canonical(cwd).context("execution cwd is unavailable")?;
        let project_root = kanzei_base::path_form::canonical(project_root)
            .context("project root is unavailable")?;
        let mut python = None;
        for root in [&cwd, &project_root] {
            let environment = root.join(".venv");
            if !environment.exists() {
                continue;
            }
            let configuration = std::fs::read(environment.join("pyvenv.cfg"))
                .context("project .venv is incomplete: pyvenv.cfg is required")?;
            let executable = if cfg!(windows) {
                environment.join("Scripts/python.exe")
            } else {
                environment.join("bin/python")
            };
            ensure!(
                executable.is_file(),
                "project Python is missing: {}",
                executable.display()
            );
            python = Some(PythonEnvironment {
                root: kanzei_base::path_form::canonical(&environment)?,
                executable_sha256: digest(std::fs::read(&executable)?),
                executable,
                configuration_sha256: digest(configuration),
            });
            break;
        }
        Ok(Self {
            harness: Some(binary_identity()?),
            source: source_snapshot(&cwd)?.map(|(identity, _)| identity),
            cwd,
            project_root,
            python,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_covers_python_clean_sources_dirty_content_and_deletions() {
        let root = std::env::temp_dir().join(format!(
            "kz-identity-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("src")).unwrap();
        assert!(git(&root, &["init", "-q"]).unwrap().status.success());
        std::fs::write(root.join("src/model.py"), "before").unwrap();
        let first = source_snapshot(&root).unwrap().unwrap().0;
        assert!(git(&root, &["add", "src/model.py"])
            .unwrap()
            .status
            .success());
        assert_eq!(
            source_snapshot(&root).unwrap().unwrap().0.fingerprint,
            first.fingerprint
        );
        std::fs::write(root.join("src/model.py"), "after").unwrap();
        let second = source_snapshot(&root).unwrap().unwrap().0;
        assert_ne!(first.fingerprint, second.fingerprint);
        std::fs::create_dir_all(root.join(".kanzei")).unwrap();
        std::fs::write(root.join(".kanzei/state.db"), "evidence").unwrap();
        assert_eq!(source_snapshot(&root).unwrap().unwrap().0, second);
        std::fs::remove_file(root.join("src/model.py")).unwrap();
        assert_ne!(
            source_snapshot(&root).unwrap().unwrap().0.fingerprint,
            second.fingerprint
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn incomplete_project_environment_is_rejected_and_complete_environment_is_bound() {
        let root = std::env::temp_dir().join(format!(
            "kz-python-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let bin = root.join(if cfg!(windows) {
            ".venv/Scripts"
        } else {
            ".venv/bin"
        });
        std::fs::create_dir_all(&bin).unwrap();
        assert!(ExecutionIdentity::capture(&root, &root).is_err());
        std::fs::write(root.join(".venv/pyvenv.cfg"), "version = 3.11.9").unwrap();
        assert!(ExecutionIdentity::capture(&root, &root).is_err());
        let python = bin.join(if cfg!(windows) {
            "python.exe"
        } else {
            "python"
        });
        std::fs::write(&python, "interpreter fixture").unwrap();
        let identity = ExecutionIdentity::capture(&root, &root).unwrap();
        assert_eq!(identity.harness.as_ref().unwrap().sha256.len(), 64);
        assert_eq!(identity.harness, Some(binary_identity().unwrap()));
        assert_eq!(
            identity.python.as_ref().unwrap().executable,
            kanzei_base::path_form::canonical(&python).unwrap()
        );
        assert_eq!(
            identity.python.as_ref().unwrap().executable_sha256,
            digest("interpreter fixture")
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
