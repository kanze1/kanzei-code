//! Search traversal and file opening share the same directory capabilities.

use kanzei_harness::permission::{
    Effect, EntryKind, FileResource, Ruleset, ScopedEntry, ScopedFile,
};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub(crate) struct SearchScope {
    cwd: PathBuf,
    project_root: PathBuf,
    rules: Option<Ruleset>,
}

impl SearchScope {
    pub(crate) fn capture(ctx: &kanzei_harness::ToolCtx) -> Self {
        Self {
            cwd: ctx.cwd.clone(),
            project_root: ctx.project_root.clone(),
            rules: kanzei_harness::permission::execution_rules(),
        }
    }
    #[cfg(test)]
    pub(crate) fn fixture(cwd: &Path) -> Self {
        Self {
            cwd: cwd.into(),
            project_root: cwd.into(),
            rules: None,
        }
    }
    pub(crate) fn file(&self, action: &str, path: &Path) -> Result<ScopedFile, String> {
        let file = ScopedFile::resolve(&self.cwd, &self.project_root, path)
            .map_err(|error| error.to_string())?;
        let resource = FileResource::resolve(&self.cwd, path).map_err(|error| error.to_string())?;
        if let Some(rules) = &self.rules {
            if rules.evaluate_file_with_rule(action, &resource).0 != Effect::Allow {
                return Err("search resource requires separate permission".into());
            }
        } else if resource.sensitivity().is_some() {
            return Err("sensitive search resource requires explicit permission".into());
        }
        Ok(file)
    }
}

struct Frame {
    directory: PathBuf,
    entries: Box<dyn Iterator<Item = std::io::Result<ScopedEntry>>>,
    ignores: Vec<ignore::gitignore::Gitignore>,
}

pub(crate) struct SearchEntry {
    path: PathBuf,
}
impl SearchEntry {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

pub(crate) struct SearchWalk {
    scope: SearchScope,
    action: &'static str,
    root: Option<PathBuf>,
    frames: Vec<Frame>,
    restricted: bool,
}

impl SearchWalk {
    pub(crate) fn new(base: &Path, action: &'static str, scope: SearchScope) -> Self {
        Self {
            scope,
            action,
            root: Some(base.into()),
            frames: Vec::new(),
            restricted: false,
        }
    }

    fn descend(
        &mut self,
        directory: &Path,
        mut ignores: Vec<ignore::gitignore::Gitignore>,
    ) -> Result<(), String> {
        let access = ScopedFile::resolve(&self.scope.cwd, &self.scope.project_root, directory)
            .map_err(|error| error.to_string())?;
        for name in [".gitignore", ".ignore"] {
            let path = directory.join(name);
            let ignore = ScopedFile::resolve(&self.scope.cwd, &self.scope.project_root, &path)
                .map_err(|error| error.to_string())?;
            match ignore.read_to_string() {
                Ok(text) => {
                    let mut builder = ignore::gitignore::GitignoreBuilder::new(directory);
                    for line in text.lines() {
                        builder
                            .add_line(Some(path.clone()), line)
                            .map_err(|error| format!("invalid ignore file: {error}"))?;
                    }
                    ignores.push(builder.build().map_err(|error| error.to_string())?);
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("cannot read search ignore rules: {error}")),
            }
        }
        let entries = access.entries().map_err(|error| error.to_string())?;
        self.frames.push(Frame {
            directory: directory.into(),
            entries: Box::new(entries),
            ignores,
        });
        Ok(())
    }

    pub(crate) fn finish(&self, mut output: String) -> String {
        if self.restricted {
            output.push_str("\n(permission restricted: some paths were excluded from this search)");
        }
        output
    }
}

impl Iterator for SearchWalk {
    type Item = Result<SearchEntry, String>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(root) = self.root.take() {
            let access = match ScopedFile::resolve(&self.scope.cwd, &self.scope.project_root, &root)
            {
                Ok(access) => access,
                Err(error) => return Some(Err(error.to_string())),
            };
            match access.metadata() {
                Ok(metadata) if metadata.is_file() => return Some(Ok(SearchEntry { path: root })),
                Ok(_) => {
                    if let Err(error) = self.descend(&root, Vec::new()) {
                        return Some(Err(error));
                    }
                }
                Err(error) => return Some(Err(error.to_string())),
            }
        }
        loop {
            let frame = self.frames.last_mut()?;
            let entry = match frame.entries.next() {
                Some(Ok(entry)) => entry,
                Some(Err(error)) => return Some(Err(format!("search traversal failed: {error}"))),
                None => {
                    self.frames.pop();
                    continue;
                }
            };
            if entry.name == ".git" || matches!(entry.kind, EntryKind::Other) {
                continue;
            }
            let path = frame.directory.join(&entry.name);
            let is_dir = matches!(entry.kind, EntryKind::Directory);
            let mut ignored = false;
            for rules in &frame.ignores {
                let matched = rules.matched_path_or_any_parents(&path, is_dir);
                if matched.is_ignore() {
                    ignored = true;
                } else if matched.is_whitelist() {
                    ignored = false;
                }
            }
            if ignored {
                continue;
            }
            let ignores = frame.ignores.clone();
            match self.scope.file(self.action, &path) {
                Ok(_) => {}
                Err(error)
                    if error.contains("FILESYSTEM_OUTSIDE_WORKSPACE")
                        || error == "search resource requires separate permission" =>
                {
                    self.restricted = true;
                    continue;
                }
                Err(error) => return Some(Err(error)),
            }
            if is_dir {
                if let Err(error) = self.descend(&path, ignores) {
                    return Some(Err(error));
                }
            } else {
                return Some(Ok(SearchEntry { path }));
            }
        }
    }
}
