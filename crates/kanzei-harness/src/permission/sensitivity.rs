//! Sensitive resources require a file-specific grant, shared by every I/O path.

use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileSensitivity {
    Credentials,
    AgentConfiguration,
}

impl FileSensitivity {
    pub fn description(self) -> &'static str {
        match self {
            Self::Credentials => "credential or private-key file",
            Self::AgentConfiguration => "agent or editor startup configuration",
        }
    }
}

pub fn file_sensitivity(path: &Path) -> Option<FileSensitivity> {
    let path = path
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    let name = path.rsplit('/').next().unwrap_or(&path);
    let suffix = |known: &str| path == known || path.ends_with(&format!("/{known}"));
    if name.starts_with(".env")
        || [
            "id_rsa",
            "id_dsa",
            "id_ecdsa",
            "id_ed25519",
            ".npmrc",
            ".pypirc",
            ".netrc",
            ".git-credentials",
            "credentials.json",
            "application_default_credentials.json",
        ]
        .contains(&name)
        || [".key", ".pem", ".p12", ".pfx", ".keystore"]
            .iter()
            .any(|extension| name.ends_with(extension))
        || [
            ".aws/credentials",
            ".azure/accesstokens.json",
            ".azure/msal_token_cache.json",
            ".docker/config.json",
            ".kube/config",
            ".codex/auth.json",
            ".claude/.credentials.json",
            ".kimi/credentials.json",
            ".kimi/credentials/kimi-code.json",
        ]
        .iter()
        .any(|known| suffix(known))
    {
        return Some(FileSensitivity::Credentials);
    }
    if name.ends_with(".code-workspace")
        || name == ".mcp.json"
        || [
            ".cursor/mcp.json",
            ".vscode/settings.json",
            ".vscode/tasks.json",
            ".vscode/launch.json",
            ".claude/settings.json",
            ".claude/settings.local.json",
            ".codex/config.toml",
            ".kanzei/kanzei.toml",
            ".git/config",
        ]
        .iter()
        .any(|known| suffix(known))
        || path.contains("/.git/hooks/")
        || path.starts_with(".git/hooks/")
    {
        return Some(FileSensitivity::AgentConfiguration);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensitive_resource_classes_preserve_action_and_path_boundaries() {
        for path in [
            ".env",
            "nested/.env.production",
            "nested/ID_ED25519",
            ".aws/credentials",
            "certs/server.KEY",
            "C:\\work\\.docker\\config.json",
        ] {
            assert_eq!(
                file_sensitivity(Path::new(path)),
                Some(FileSensitivity::Credentials),
                "{path}"
            );
        }
        for path in [
            ".cursor/mcp.json",
            "nested/.vscode/settings.json",
            "project.code-workspace",
            ".codex/config.toml",
        ] {
            let sensitivity = file_sensitivity(Path::new(path)).unwrap();
            assert_eq!(sensitivity, FileSensitivity::AgentConfiguration);
        }
        for path in [
            "src/environment.rs",
            "public/key.svg",
            "not.aws/credentials",
            "docs/settings.json",
        ] {
            assert_eq!(file_sensitivity(Path::new(path)), None, "{path}");
        }
    }
}
