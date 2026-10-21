use super::*;
pub use kanzei_harness::execution::SourceFile;
use kanzei_harness::execution::{executable, source_snapshot};

pub(super) fn freeze(root: &Path, target: &Path) -> anyhow::Result<(Vec<SourceFile>, String)> {
    ensure!(!target.exists(), "snapshot destination already exists");
    let (identity, manifest) =
        source_snapshot(root)?.context("verification snapshots require a Git working tree")?;
    ensure!(!manifest.is_empty(), "no source files to freeze");
    std::fs::create_dir_all(target)?;
    for entry in &manifest {
        let source = root.join(&entry.path);
        let file = target.join(&entry.path);
        std::fs::create_dir_all(file.parent().context("snapshot parent missing")?)?;
        std::fs::copy(&source, &file)?;
        std::fs::set_permissions(&file, std::fs::metadata(&source)?.permissions())?;
    }
    ensure!(
        source_snapshot(root)?
            .is_some_and(|(current, _)| current.fingerprint == identity.fingerprint)
            && intact(target, &manifest)?,
        "source changed while freezing; retry after the write finishes"
    );
    Ok((manifest, identity.fingerprint))
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
