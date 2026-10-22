//! One source scan for tolerant views, strict reads and integrity diagnostics.
use std::path::{Path, PathBuf};

use anyhow::Context;

use super::super::{parse_entry, MemoryEntry};

pub(in crate::memory) struct SourceScan {
    pub(in crate::memory) entries: Vec<(PathBuf, MemoryEntry)>,
    pub(in crate::memory) issues: Vec<anyhow::Error>,
}

impl SourceScan {
    pub(super) fn extend(&mut self, other: Self) {
        self.entries.extend(other.entries);
        self.issues.extend(other.issues);
    }

    pub(super) fn into_checked(self) -> anyhow::Result<Vec<(PathBuf, MemoryEntry)>> {
        if let Some(issue) = self.issues.into_iter().next() {
            return Err(issue);
        }
        Ok(self.entries)
    }
}

pub(super) fn scan(root: &Path) -> SourceScan {
    let mut scan = SourceScan {
        entries: Vec::new(),
        issues: Vec::new(),
    };
    let directory = match std::fs::read_dir(root) {
        Ok(directory) => directory,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return scan,
        Err(error) => {
            scan.issues.push(
                anyhow::Error::new(error)
                    .context(format!("read memory directory {}", root.display())),
            );
            return scan;
        }
    };
    for item in directory {
        let path = match item.with_context(|| format!("list memory directory {}", root.display())) {
            Ok(item) => item.path(),
            Err(error) => {
                scan.issues.push(error);
                continue;
            }
        };
        if path.extension().and_then(|extension| extension.to_str()) != Some("md")
            || matches!(
                path.file_name().and_then(|name| name.to_str()),
                Some("INDEX.md" | "inbox.md" | "voided-ids.md")
            )
        {
            continue;
        }
        let text = match std::fs::read_to_string(&path)
            .with_context(|| format!("read memory source {}", path.display()))
        {
            Ok(text) => text,
            Err(error) => {
                scan.issues.push(error);
                continue;
            }
        };
        let entry = parse_entry(&text);
        if entry.id.is_empty() {
            scan.issues.push(anyhow::anyhow!(
                "memory source {} has no id in frontmatter",
                path.display()
            ));
        } else {
            scan.entries.push((path, entry));
        }
    }
    scan.entries
        .sort_by(|a, b| (&a.1.id, &a.0).cmp(&(&b.1.id, &b.0)));
    scan.issues.sort_by_key(ToString::to_string);
    scan
}
