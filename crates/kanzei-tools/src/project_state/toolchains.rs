//! Bounded executable discovery. A miss is not proof that a tool is uninstalled.
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use super::{Discovery, Toolchain};

#[derive(Default)]
pub(super) struct Locations {
    pub sdk: Vec<(String, PathBuf)>,
    pub user_flutter: Vec<PathBuf>,
}

impl Locations {
    pub fn from_environment() -> Self {
        let mut locations = Self::default();
        for (variable, tools) in [
            ("FLUTTER_ROOT", &["flutter", "dart"][..]),
            ("DART_SDK", &["dart"][..]),
            ("JAVA_HOME", &["java"][..]),
            ("CARGO_HOME", &["cargo", "rustc"][..]),
            ("GOROOT", &["go"][..]),
        ] {
            if let Some(root) = std::env::var_os(variable).filter(|v| !v.is_empty()) {
                for tool in tools {
                    locations
                        .sdk
                        .push(((*tool).into(), PathBuf::from(&root).join("bin")));
                }
            }
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            let local = PathBuf::from(local);
            locations.user_flutter.extend([
                local.join("Programs/flutter-sdk/flutter"),
                local.join("Programs/flutter"),
                local.join("flutter"),
            ]);
        }
        if let Some(home) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) {
            let home = PathBuf::from(home);
            locations.user_flutter.extend([
                home.join("flutter"),
                home.join("development/flutter"),
                home.join("scoop/apps/flutter/current"),
            ]);
        }
        locations
    }
}

fn in_directory(name: &str, dir: &Path) -> Option<PathBuf> {
    crate::shell::find_executable(name, &std::env::join_paths([dir]).ok()?)
}

fn project_flutter_bins(root: &Path) -> Vec<PathBuf> {
    let mut bins = vec![root.join(".fvm/flutter_sdk/bin")];
    let config = root.join("android/local.properties");
    if std::fs::metadata(&config).is_ok_and(|m| m.len() <= 65536) {
        if let Ok(text) = std::fs::read_to_string(config) {
            for line in text.lines() {
                if let Some((key, value)) = line.split_once('=') {
                    if key.trim() == "flutter.sdk" {
                        let value = value.trim().replace("\\\\", "\\").replace("\\:", ":");
                        if !value.is_empty() {
                            let sdk = PathBuf::from(value);
                            bins.push(
                                if sdk.is_absolute() {
                                    sdk
                                } else {
                                    root.join(sdk)
                                }
                                .join("bin"),
                            );
                        }
                    }
                }
            }
        }
    }
    bins
}

pub(super) fn discover(root: &Path, name: &str, path: &OsStr, locations: &Locations) -> Toolchain {
    let mut found = crate::shell::find_executable(name, path).map(|p| (p, Discovery::Path));
    let flutter_tool = matches!(name, "flutter" | "dart");
    if found.is_none() && flutter_tool {
        found = project_flutter_bins(root)
            .iter()
            .find_map(|bin| in_directory(name, bin).map(|p| (p, Discovery::Project)));
    }
    if found.is_none() {
        found = locations
            .sdk
            .iter()
            .filter(|(tool, _)| tool == name)
            .find_map(|(_, bin)| in_directory(name, bin).map(|p| (p, Discovery::SdkEnvironment)));
    }
    if found.is_none() && name == "dart" {
        found = crate::shell::find_executable("flutter", path)
            .and_then(|p| in_directory(name, p.parent()?))
            .map(|p| (p, Discovery::SdkSibling));
    }
    if found.is_none() && flutter_tool {
        found = locations.user_flutter.iter().find_map(|sdk| {
            in_directory(name, &sdk.join("bin")).map(|p| (p, Discovery::UserInstall))
        });
    }
    Toolchain {
        name: name.into(),
        source: found
            .as_ref()
            .map_or(Discovery::NotFound, |(_, source)| *source),
        found: found.map(|(p, _)| p.display().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binary(dir: &Path, name: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let file = dir.join(if cfg!(windows) {
            format!("{name}.bat")
        } else {
            name.into()
        });
        std::fs::write(&file, "").unwrap();
        file
    }

    #[test]
    fn finds_user_flutter_and_dart_outside_path_without_claiming_version_success() {
        let root = super::super::tests::temp_root("sdk-outside-path");
        let sdk = root.join("Programs/flutter-sdk/flutter");
        let flutter = binary(&sdk.join("bin"), "flutter");
        let dart = binary(&sdk.join("bin"), "dart");
        let locations = Locations {
            user_flutter: vec![sdk],
            ..Default::default()
        };
        for (name, expected) in [("flutter", flutter), ("dart", dart)] {
            let result = discover(&root, name, OsStr::new(""), &locations);
            assert_eq!(result.found, Some(expected.display().to_string()));
            assert_eq!(result.source, Discovery::UserInstall);
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn uses_project_sdk_and_ignores_stale_sdk_locations() {
        let root = super::super::tests::temp_root("sdk-config");
        let sdk = root.join("custom-sdk");
        let expected = binary(&sdk.join("bin"), "flutter");
        std::fs::create_dir_all(root.join("android")).unwrap();
        std::fs::write(
            root.join("android/local.properties"),
            format!(
                "flutter.sdk={}\n",
                sdk.display().to_string().replace('\\', "\\\\")
            ),
        )
        .unwrap();
        let result = discover(&root, "flutter", OsStr::new(""), &Locations::default());
        assert_eq!(result.source, Discovery::Project);
        assert_eq!(result.found, Some(expected.display().to_string()));
        std::fs::remove_file(expected).unwrap();
        let result = discover(&root, "flutter", OsStr::new(""), &Locations::default());
        assert_eq!(result.source, Discovery::NotFound);
        assert!(result.found.is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn path_wins_and_environment_sdk_is_a_fallback() {
        let root = super::super::tests::temp_root("sdk-precedence");
        let path_bin = root.join("path-bin");
        let path_exe = binary(&path_bin, "dart");
        let sdk_bin = root.join("sdk-bin");
        let sdk_exe = binary(&sdk_bin, "dart");
        let locations = Locations {
            sdk: vec![("dart".into(), sdk_bin)],
            ..Default::default()
        };
        let path = std::env::join_paths([path_bin]).unwrap();
        assert_eq!(
            discover(&root, "dart", &path, &locations).found,
            Some(path_exe.display().to_string())
        );
        let result = discover(&root, "dart", OsStr::new(""), &locations);
        assert_eq!(result.found, Some(sdk_exe.display().to_string()));
        assert_eq!(result.source, Discovery::SdkEnvironment);
        std::fs::remove_dir_all(root).unwrap();
    }
}
