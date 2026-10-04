//! Global skill catalog shared by every conversation and the desktop manager.
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use crate::{harness::HarnessDraft, SkillDef};

pub const BUILTINS: &[(&str, &str, &str)] = &[
    (
        "skill-creator",
        "技能创建",
        include_str!("../skills/skill-creator/SKILL.md"),
    ),
    (
        "code-review",
        "代码审查",
        include_str!("../skills/code-review/SKILL.md"),
    ),
    (
        "debugging",
        "问题调试",
        include_str!("../skills/debugging/SKILL.md"),
    ),
    (
        "frontend-design",
        "网页设计",
        include_str!("../skills/frontend-design/SKILL.md"),
    ),
    (
        "web-research",
        "资料研究",
        include_str!("../skills/web-research/SKILL.md"),
    ),
    (
        "documents",
        "Word 文档",
        include_str!("../skills/documents/SKILL.md"),
    ),
    (
        "spreadsheets",
        "电子表格",
        include_str!("../skills/spreadsheets/SKILL.md"),
    ),
    (
        "presentations",
        "演示文稿",
        include_str!("../skills/presentations/SKILL.md"),
    ),
    ("pdf", "PDF 处理", include_str!("../skills/pdf/SKILL.md")),
];

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct SkillPreferences {
    #[serde(default)]
    pub disabled: BTreeSet<String>,
}

pub fn global_home() -> anyhow::Result<PathBuf> {
    crate::kanzei_home().ok_or_else(|| anyhow::anyhow!("无法确定全局 Skills 目录"))
}

pub fn compatibility_home() -> Option<PathBuf> {
    if std::env::var_os("KANZEI_HOME").is_some() {
        None
    } else {
        dirs::home_dir()
    }
}

pub fn preferences_at(home: &Path) -> anyhow::Result<SkillPreferences> {
    let path = home.join("skills.json");
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(serde_json::from_str(&text)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(SkillPreferences::default())
        }
        Err(error) => Err(error.into()),
    }
}

pub fn set_enabled_at(home: &Path, name: &str, enabled: bool) -> anyhow::Result<()> {
    let path = home.join("skills.json");
    let _lock = kanzei_base::atomic_file::lock_exclusive(&path)?;
    let mut prefs = preferences_at(home)?;
    if enabled {
        prefs.disabled.remove(name);
    } else {
        prefs.disabled.insert(name.to_owned());
    }
    kanzei_base::atomic_file::write_atomic(&path, &serde_json::to_string_pretty(&prefs)?)?;
    Ok(())
}

pub fn install_builtins_at(home: &Path) -> anyhow::Result<()> {
    for (name, _, text) in BUILTINS {
        let path = home.join("builtin-skills").join(name).join("SKILL.md");
        if std::fs::read_to_string(&path).ok().as_deref() != Some(text) {
            let _lock = kanzei_base::atomic_file::lock_exclusive(&path)?;
            kanzei_base::atomic_file::write_atomic(&path, text)?;
        }
    }
    Ok(())
}

/// Project directories deliberately do not participate: skills are personal and global.
pub fn catalog_at(home: &Path, user_home: Option<&Path>) -> anyhow::Result<Vec<SkillDef>> {
    install_builtins_at(home)?;
    let mut draft = HarnessDraft::default();
    if let Some(user) = user_home {
        for base in [".codex", ".claude", ".agents"] {
            crate::markdown::scan_skills(&user.join(base).join("skills"), &mut draft);
        }
    }
    crate::markdown::scan_skills(&home.join("builtin-skills"), &mut draft);
    crate::markdown::scan_skills(&home.join("skills"), &mut draft);
    Ok(draft
        .skills
        .iter()
        .map(|(_, skill)| skill.clone())
        .collect())
}

pub fn global_catalog() -> anyhow::Result<Vec<SkillDef>> {
    catalog_at(&global_home()?, compatibility_home().as_deref())
}

pub fn enabled_catalog() -> anyhow::Result<Vec<SkillDef>> {
    let home = global_home()?;
    let prefs = preferences_at(&home)?;
    Ok(catalog_at(&home, compatibility_home().as_deref())?
        .into_iter()
        .filter(|skill| !prefs.disabled.contains(&skill.name))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn builtins_and_personal_preferences_survive_reload() {
        let home = std::env::temp_dir().join(format!(
            "kz-global-skills-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let catalog = catalog_at(&home, None).unwrap();
        assert_eq!(catalog.len(), 9);
        assert!(catalog
            .iter()
            .all(|skill| skill.path.is_file() && !skill.description.is_empty()));
        set_enabled_at(&home, "pdf", false).unwrap();
        set_enabled_at(&home, "documents", false).unwrap();
        assert_eq!(preferences_at(&home).unwrap().disabled.len(), 2);
        install_builtins_at(&home).unwrap();
        assert!(preferences_at(&home).unwrap().disabled.contains("pdf"));
        set_enabled_at(&home, "pdf", true).unwrap();
        assert!(!preferences_at(&home).unwrap().disabled.contains("pdf"));
        std::fs::write(home.join("skills.json"), "invalid").unwrap();
        assert!(preferences_at(&home).is_err());
        std::fs::remove_dir_all(home).unwrap();
    }
}
