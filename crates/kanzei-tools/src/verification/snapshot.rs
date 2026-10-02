use super::*;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceFile {
    pub path: String,
    pub sha256: String,
    pub executable: bool,
}

fn files(root: &Path) -> anyhow::Result<Vec<String>> {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .output()?;
    ensure!(
        output.status.success(),
        "verification snapshots currently require a Git working tree"
    );
    let names = String::from_utf8(output.stdout)?;
    let mut paths = BTreeSet::new();
    for name in names.split('\0').filter(|p| !p.is_empty()) {
        let rel = Path::new(name);
        ensure!(
            rel.components()
                .all(|c| matches!(c, std::path::Component::Normal(_))),
            "invalid source path"
        );
        if name == ".git"
            || name.starts_with(".git/")
            || name == ".kanzei"
            || name.starts_with(".kanzei/")
        {
            continue;
        }
        match std::fs::symlink_metadata(root.join(rel)) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
            Ok(meta) => ensure!(
                meta.is_file() && !meta.file_type().is_symlink(),
                "snapshot requires regular files; unsupported path: {name}"
            ),
        }
        // 所有父目录也须留在源树内，不能通过目录链接隐式读取外部依赖。
        ensure!(
            root.join(rel).canonicalize()?.starts_with(root),
            "source path escapes working tree: {name}"
        );
        paths.insert(name.into());
    }
    ensure!(!paths.is_empty(), "no source files to freeze");
    Ok(paths.into_iter().collect())
}

fn executable(path: &Path) -> anyhow::Result<bool> {
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

pub(super) fn freeze(root: &Path, target: &Path) -> anyhow::Result<(Vec<SourceFile>, String)> {
    ensure!(!target.exists(), "snapshot destination already exists");
    let paths = files(root)?;
    ensure!(paths.len() <= 30_000, "snapshot has more than 30000 files");
    std::fs::create_dir_all(target)?;
    let mut total = 0u64;
    let mut manifest = Vec::new();
    for name in &paths {
        let source = root.join(name);
        let bytes = std::fs::read(&source)?;
        total += bytes.len() as u64;
        ensure!(
            total <= 512 * 1024 * 1024,
            "snapshot exceeds 512 MiB; narrow the source tree"
        );
        let file = target.join(name);
        std::fs::create_dir_all(file.parent().context("snapshot parent missing")?)?;
        std::fs::write(&file, &bytes)?;
        std::fs::set_permissions(&file, std::fs::metadata(&source)?.permissions())?;
        manifest.push(SourceFile {
            path: name.clone(),
            sha256: hash(&bytes),
            executable: executable(&source)?,
        });
    }
    ensure!(
        files(root)? == paths && intact(root, &manifest)?,
        "source changed while freezing; retry after the write finishes"
    );
    let fingerprint = hash(serde_json::to_vec(&manifest)?);
    Ok((manifest, fingerprint))
}

pub(super) fn intact(root: &Path, manifest: &[SourceFile]) -> anyhow::Result<bool> {
    for entry in manifest {
        let file = root.join(&entry.path);
        if !file.is_file()
            || hash(std::fs::read(&file)?) != entry.sha256
            || executable(&file)? != entry.executable
        {
            return Ok(false);
        }
    }
    Ok(true)
}
