//! Global skill catalog shared by every conversation and the desktop manager.
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use crate::SkillDef;

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

pub fn is_enabled(skill: &SkillDef, preferences: &SkillPreferences) -> bool {
    !preferences.disabled.contains(&skill.id) && !preferences.disabled.contains(&skill.name)
}

/// Bare names are supported only when unambiguous. Source identity never
/// depends on directory enumeration order or which provider loaded last.
pub fn resolve<'a>(skills: &'a [SkillDef], selector: &str) -> anyhow::Result<&'a SkillDef> {
    if let Some(skill) = skills.iter().find(|skill| skill.id == selector) {
        return Ok(skill);
    }
    let matches: Vec<_> = skills
        .iter()
        .filter(|skill| skill.name == selector)
        .collect();
    match matches.as_slice() {
        [skill] => Ok(skill),
        [] => anyhow::bail!("Skill 不存在：{selector}"),
        _ => anyhow::bail!(
            "Skill 名称不唯一，请使用 {}",
            matches
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>()
                .join(" / ")
        ),
    }
}

pub fn read_content(skill: &SkillDef) -> anyhow::Result<String> {
    let content = match &skill.embedded_content {
        Some(content) => content.clone(),
        None => std::fs::read_to_string(&skill.path)?,
    };
    anyhow::ensure!(content.len() <= 131_072, "Skill 太大：{}", skill.id);
    Ok(content)
}

pub fn set_enabled_at(home: &Path, selector: &str, enabled: bool) -> anyhow::Result<()> {
    let catalog = catalog_at(home, compatibility_home().as_deref())?;
    let skill = resolve(&catalog, selector)?;
    let path = home.join("skills.json");
    let _lock = kanzei_base::atomic_file::lock_exclusive(&path)?;
    let mut prefs = preferences_at(home)?;
    // Convert legacy unqualified disabled names to every matching identity,
    // preserving their disabled state before changing one explicit selection.
    for legacy in prefs
        .disabled
        .clone()
        .into_iter()
        .filter(|name| !name.contains(':'))
    {
        prefs.disabled.remove(&legacy);
        prefs.disabled.extend(
            catalog
                .iter()
                .filter(|s| s.name == legacy)
                .map(|s| s.id.clone()),
        );
    }
    if enabled {
        prefs.disabled.remove(&skill.id);
    } else {
        prefs.disabled.insert(skill.id.clone());
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
    let mut catalog = Vec::new();
    for (name, _, content) in BUILTINS {
        let metadata = crate::markdown::parse_frontmatter(content);
        catalog.push(SkillDef {
            id: format!("builtin:{name}"),
            source: "builtin".into(),
            name: (*name).into(),
            description: metadata.get("description").unwrap_or("").into(),
            path: PathBuf::from(format!("builtin:{name}/SKILL.md")),
            embedded_content: Some((*content).into()),
            disable_model_invocation: false,
            user_invocable: true,
        });
    }
    let mut directories = vec![("personal", home.join("skills"))];
    if let Some(user) = user_home {
        for (source, base) in [
            ("codex", ".codex"),
            ("claude", ".claude"),
            ("agents", ".agents"),
        ] {
            directories.push((source, user.join(base).join("skills")));
        }
    }
    for (source, directory) in directories {
        for mut skill in crate::markdown::skills_in_directory(&directory) {
            skill.id = format!("{source}:{}", skill.name);
            skill.source = source.into();
            anyhow::ensure!(
                !catalog.iter().any(|existing| existing.id == skill.id),
                "同一来源中存在重复 Skill：{}",
                skill.id
            );
            catalog.push(skill);
        }
    }
    catalog.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(catalog)
}

pub fn global_catalog() -> anyhow::Result<Vec<SkillDef>> {
    catalog_at(&global_home()?, compatibility_home().as_deref())
}

pub fn enabled_catalog() -> anyhow::Result<Vec<SkillDef>> {
    let home = global_home()?;
    let prefs = preferences_at(&home)?;
    Ok(catalog_at(&home, compatibility_home().as_deref())?
        .into_iter()
        .filter(|skill| is_enabled(skill, &prefs))
        .collect())
}

/// A resource reader keeps packaged and filesystem skills behind one API.
/// Listing the catalog performs no installation or repair writes.
pub struct SkillReadTool;

#[async_trait::async_trait]
impl crate::Tool for SkillReadTool {
    fn read_only_support(&self) -> crate::tool::ReadOnlySupport {
        crate::tool::ReadOnlySupport::ReadOnly
    }
    fn name(&self) -> &'static str {
        "skill_read"
    }
    fn description(&self) -> String {
        "Read the instructions of an available skill by source-qualified id.".into()
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({"type":"object","required":["id"],"properties":{"id":{"type":"string"}},"additionalProperties":false})
    }
    fn concurrency(&self, _: &serde_json::Value, ctx: &crate::ToolCtx) -> crate::ToolConcurrency {
        crate::ToolConcurrency::shared_worktree(ctx)
    }
    async fn execute(&self, input: serde_json::Value, _: &crate::ToolCtx) -> crate::ToolOutput {
        let result = (|| -> anyhow::Result<String> {
            let selector = input
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("missing id"))?;
            let catalog = enabled_catalog()?;
            let skill = resolve(&catalog, selector)?;
            anyhow::ensure!(
                !skill.disable_model_invocation,
                "Skill 仅允许用户显式调用：{}",
                skill.id
            );
            Ok(format!(
                "Skill {}\nSource: {}\n{}",
                skill.id,
                skill.path.display(),
                read_content(skill)?
            ))
        })();
        match result {
            Ok(content) => crate::ToolOutput::ok(content),
            Err(error) => crate::ToolOutput::error(error.to_string()),
        }
    }
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
            .all(|skill| read_content(skill).is_ok() && !skill.description.is_empty()));
        assert!(
            !home.exists(),
            "catalog reads must not create the global home"
        );
        set_enabled_at(&home, "builtin:pdf", false).unwrap();
        set_enabled_at(&home, "builtin:documents", false).unwrap();
        assert_eq!(preferences_at(&home).unwrap().disabled.len(), 2);
        install_builtins_at(&home).unwrap();
        assert!(preferences_at(&home)
            .unwrap()
            .disabled
            .contains("builtin:pdf"));
        set_enabled_at(&home, "builtin:pdf", true).unwrap();
        assert!(!preferences_at(&home)
            .unwrap()
            .disabled
            .contains("builtin:pdf"));
        std::fs::write(home.join("skills.json"), "invalid").unwrap();
        assert!(preferences_at(&home).is_err());
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn same_named_sources_are_independent_and_bare_names_are_ambiguous() {
        let home = std::env::temp_dir().join(format!(
            "kz-skill-identities-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(home.join("skills/pdf")).unwrap();
        std::fs::write(
            home.join("skills/pdf/SKILL.md"),
            "---\nname: pdf\ndescription: personal PDF\n---\npersonal instructions",
        )
        .unwrap();
        let skills = catalog_at(&home, None).unwrap();
        assert!(resolve(&skills, "pdf").is_err());
        assert_eq!(resolve(&skills, "personal:pdf").unwrap().source, "personal");
        assert!(read_content(resolve(&skills, "builtin:pdf").unwrap())
            .unwrap()
            .contains("PDF"));
        std::fs::write(home.join("skills.json"), r#"{"disabled":["pdf"]}"#).unwrap();
        set_enabled_at(&home, "builtin:pdf", true).unwrap();
        let prefs = preferences_at(&home).unwrap();
        assert!(is_enabled(resolve(&skills, "builtin:pdf").unwrap(), &prefs));
        assert!(!is_enabled(
            resolve(&skills, "personal:pdf").unwrap(),
            &prefs
        ));
        assert!(!home.join("builtin-skills").exists());
        std::fs::remove_dir_all(home).unwrap();
    }
}
