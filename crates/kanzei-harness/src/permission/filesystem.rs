//! Filesystem identities used by both preflight and dynamically discovered files.

use std::path::{Path, PathBuf};

use super::{normalize_resource, wildcard_match, Effect, Rule, Ruleset};

/// Literal resources retain their action-specific semantics. File resources are
/// checked against both their requested spelling and their physical target.
pub enum Resource {
    Literal(String),
    File(String),
}

pub struct FileResource {
    pub name: String,
    identities: Vec<String>,
}

impl FileResource {
    pub fn sensitivity(&self) -> Option<super::FileSensitivity> {
        self.identities
            .iter()
            .filter_map(|identity| super::file_sensitivity(Path::new(identity)))
            .next()
    }
    pub fn resolve(cwd: &Path, path: &Path) -> std::io::Result<Self> {
        let absolute = cwd.join(path);
        let physical = resolve_target(&absolute)?;
        let physical_cwd = kanzei_base::path_form::canonical(cwd)?;
        let mut identities = Vec::new();
        for (path, root) in [(&absolute, cwd), (&physical, physical_cwd.as_path())] {
            for identity in [path.as_path(), path.strip_prefix(root).unwrap_or(path)] {
                let raw = identity.to_string_lossy();
                let key = normalize_resource(&kanzei_base::path_form::strip_verbatim(&raw));
                #[cfg(windows)]
                let key = key.to_lowercase();
                let key = if key.is_empty() { ".".into() } else { key };
                if !identities.contains(&key) {
                    identities.push(key);
                }
            }
        }
        let name = identities.last().cloned().unwrap();
        Ok(Self { name, identities })
    }

    fn matches(&self, action: &str, rule: &Rule) -> bool {
        let pattern = normalize_resource(&rule.resource);
        #[cfg(windows)]
        let pattern = pattern.to_lowercase();
        wildcard_match(&rule.action, action)
            && self
                .identities
                .iter()
                .any(|identity| wildcard_match(&pattern, identity))
    }
}

// Missing leaves still have a physical parent (write and missing-path errors).
// Other resolution failures must remain visible instead of granting lexical access.
pub(crate) fn resolve_target(path: &Path) -> std::io::Result<PathBuf> {
    // Resolve each parent traversal against its physical prefix before removing
    // it, including missing write parents. Keep symlink/.. semantics consistent
    // with the identity that is authorized and opened.
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                normalized = resolve_target(&normalized)?;
                normalized.pop();
            }
            std::path::Component::CurDir => {}
            component => normalized.push(component.as_os_str()),
        }
    }
    let path = normalized.as_path();
    match kanzei_base::path_form::canonical(path) {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let Some(parent) = path.parent() else {
                return Err(error);
            };
            let Some(name) = path.file_name() else {
                return Err(error);
            };
            Ok(resolve_target(parent)?.join(name))
        }
        Err(error) => Err(error),
    }
}

impl Ruleset {
    /// Match all identities in one ordered policy evaluation. Evaluating each
    /// spelling independently would turn relative allow rules into spurious Ask.
    pub fn evaluate_file_with_rule(
        &self,
        action: &str,
        file: &FileResource,
    ) -> (Effect, Option<&Rule>) {
        if self
            .hard_denies
            .iter()
            .any(|rule| file.matches(action, rule))
        {
            return (Effect::Deny, None);
        }
        let rule = self
            .rules
            .iter()
            .rev()
            .find(|rule| file.matches(action, rule));
        let effect = rule.map_or(Effect::Ask, |rule| rule.effect);
        if effect == Effect::Allow
            && file.sensitivity().is_some()
            && rule.is_some_and(|rule| rule.resource.contains('*'))
        {
            return (Effect::Ask, rule);
        }
        (effect, rule)
    }
}

/// Capture before spawn_blocking: Tokio task locals do not cross that boundary.
pub fn execution_rules() -> Option<Ruleset> {
    super::EXECUTION_RULES.try_with(Clone::clone).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broad_allows_do_not_grant_sensitive_files_but_exact_consent_does() {
        let root = std::env::temp_dir().join(format!(
            "kz-sensitive-policy-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".vscode")).unwrap();
        let mut rules = Ruleset::new(vec![crate::rule("*", "*", Effect::Allow)]);
        for (action, path) in [
            ("read", ".env"),
            ("grep", ".env"),
            ("write", ".vscode/settings.json"),
        ] {
            let file = FileResource::resolve(&root, Path::new(path)).unwrap();
            assert!(file.sensitivity().is_some());
            assert_eq!(
                rules.evaluate_file_with_rule(action, &file).0,
                Effect::Ask,
                "{action} {path}"
            );
            rules.push(crate::rule(action, path, Effect::Allow));
            assert_eq!(
                rules.evaluate_file_with_rule(action, &file).0,
                Effect::Allow
            );
            rules.push_hard_deny(crate::rule(action, path, Effect::Deny));
            assert_eq!(rules.evaluate_file_with_rule(action, &file).0, Effect::Deny);
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn file_identity_preserves_relative_allow_absolute_deny_and_hard_deny_precedence() {
        let root = std::env::temp_dir().join(format!(
            "kz-file-identity-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("nested")).unwrap();
        std::fs::write(root.join("nested/allowed.txt"), "allowed").unwrap();
        let mut rules = Ruleset::new(vec![crate::rule(
            "read",
            "nested/allowed.txt",
            Effect::Allow,
        )]);
        for requested in [
            root.join("nested/allowed.txt"),
            PathBuf::from("nested/allowed.txt"),
        ] {
            let file = FileResource::resolve(&root, &requested).unwrap();
            assert_eq!(
                rules.evaluate_file_with_rule("read", &file).0,
                Effect::Allow
            );
        }
        rules.push(crate::rule(
            "read",
            &root.join("nested/allowed.txt").to_string_lossy(),
            Effect::Deny,
        ));
        let file = FileResource::resolve(&root, Path::new("nested/allowed.txt")).unwrap();
        assert_eq!(rules.evaluate_file_with_rule("read", &file).0, Effect::Deny);
        rules.push(crate::rule("read", "nested/allowed.txt", Effect::Allow));
        assert_eq!(
            rules.evaluate_file_with_rule("read", &file).0,
            Effect::Allow
        );
        rules.push_hard_deny(crate::rule("read", "nested/*", Effect::Deny));
        assert_eq!(rules.evaluate_file_with_rule("read", &file).0, Effect::Deny);
        let missing = FileResource::resolve(&root, Path::new("nested/new.txt")).unwrap();
        assert_eq!(
            rules.evaluate_file_with_rule("read", &missing).0,
            Effect::Deny
        );
        #[cfg(windows)]
        {
            std::fs::write(root.join("Secret.TXT"), "secret").unwrap();
            let file = FileResource::resolve(&root, Path::new("SECRET.txt")).unwrap();
            rules.push(crate::rule("read", "secret.txt", Effect::Deny));
            assert_eq!(rules.evaluate_file_with_rule("read", &file).0, Effect::Deny);
            rules.push(crate::rule("read", "SECRET.TXT", Effect::Allow));
            assert_eq!(
                rules.evaluate_file_with_rule("read", &file).0,
                Effect::Allow
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
