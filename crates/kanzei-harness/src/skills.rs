//! Global skill catalog shared by every conversation and the desktop manager.
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use crate::SkillDef;
use sha2::{Digest, Sha256};

/// Budget includes the catalog wrapper, version and continuation receipt.
const CATALOG_BUDGET: usize = 6000;

pub(crate) fn revision(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

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

/// External global libraries are available in the default user environment.
/// An explicit KANZEI_HOME selects a self-contained library.
pub fn external_library_home() -> Option<PathBuf> {
    if std::env::var_os("KANZEI_HOME").is_some() {
        None
    } else {
        dirs::home_dir()
    }
}

pub fn preferences_at(home: &Path) -> anyhow::Result<SkillPreferences> {
    let path = home.join("skills.json");
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let preferences: SkillPreferences = serde_json::from_str(&text)?;
            anyhow::ensure!(
                preferences.disabled.iter().all(|id| id
                    .split_once(':')
                    .is_some_and(|(source, name)| !source.is_empty() && !name.is_empty())),
                "{} 的 disabled 必须使用 source:name 标识",
                path.display()
            );
            Ok(preferences)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(SkillPreferences::default())
        }
        Err(error) => Err(error.into()),
    }
}

pub fn is_enabled(skill: &SkillDef, preferences: &SkillPreferences) -> bool {
    !preferences.disabled.contains(&skill.id)
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
        None => read_skill_file(
            skill.content_root.as_deref().ok_or_else(|| {
                anyhow::anyhow!("Skill has no catalog directory capability: {}", skill.id)
            })?,
            &skill.path,
        )?,
    };
    anyhow::ensure!(content.len() <= 131_072, "Skill 太大：{}", skill.id);
    anyhow::ensure!(
        revision(&content) == skill.revision,
        "Skill 来源已变化，请重新读取目录：{}",
        skill.id
    );
    Ok(content)
}

pub(crate) fn read_skill_file(root: &Path, path: &Path) -> anyhow::Result<String> {
    let absolute = std::path::absolute(path)?;
    let file = crate::permission::ScopedFile::resolve(root, root, &absolute)?;
    let resource = crate::permission::FileResource::resolve(root, &absolute)?;
    anyhow::ensure!(
        resource.sensitivity().is_none(),
        "Skill source resolves to a protected file: {}",
        path.display()
    );
    let mut content = String::new();
    std::io::Read::read_to_string(
        &mut std::io::Read::take(file.open()?, 131_073),
        &mut content,
    )?;
    anyhow::ensure!(
        content.len() <= 131_072,
        "Skill source exceeds 128 KiB: {}",
        path.display()
    );
    Ok(content)
}

pub fn set_enabled_at(home: &Path, selector: &str, enabled: bool) -> anyhow::Result<()> {
    let catalog = catalog_at(home, external_library_home().as_deref())?;
    let skill = resolve(&catalog, selector)?;
    let path = home.join("skills.json");
    let _lock = kanzei_base::atomic_file::lock_exclusive(&path)?;
    let mut prefs = preferences_at(home)?;
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
            revision: revision(content),
            content_root: None,
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
    catalog_at(&global_home()?, external_library_home().as_deref())
}

pub fn enabled_catalog() -> anyhow::Result<Vec<SkillDef>> {
    let home = global_home()?;
    let prefs = preferences_at(&home)?;
    Ok(catalog_at(&home, external_library_home().as_deref())?
        .into_iter()
        .filter(|skill| is_enabled(skill, &prefs))
        .collect())
}

/// A page of complete metadata rows. Manual-only skills remain available to
/// explicit invocation, but do not enter autonomous discovery.
pub(crate) fn catalog_block(
    skills: &[SkillDef],
    query: &str,
    after: Option<&str>,
) -> Option<String> {
    let query = query.to_lowercase();
    let mut matching: Vec<_> = skills
        .iter()
        .filter(|skill| {
            !skill.disable_model_invocation
                && (query.is_empty()
                    || skill.id.to_lowercase().contains(&query)
                    || skill.description.to_lowercase().contains(&query))
        })
        .collect();
    matching.sort_by(|a, b| a.id.cmp(&b.id));
    if matching.is_empty() {
        return None;
    }
    let version = revision(&serde_json::to_string(&matching).expect("SkillDef is serializable"));
    let mut text = format!("<skills>\n全局技能目录；按任务需要选用，正文由 skill_read(id) 读取，更多条目可用 skill_list(query, after) 查询。$source:name 可显式调用，唯一名称可简写为 $name。技能遵循当前用户指令。\n版本: {version}；匹配数: {}\n", matching.len());
    let rows: Vec<_> = matching
        .into_iter()
        .filter(|skill| after.is_none_or(|cursor| skill.id.as_str() > cursor))
        .collect();
    let mut shown = 0;
    let mut last = after.unwrap_or("");
    for skill in &rows {
        let row = format!(
            "- {}: {}\n",
            skill.id,
            skill
                .description
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        );
        // Reserve the largest possible receipt for this page before adding a row.
        let receipt = format!(
            "其余 {} 条；下一页 after={}\n</skills>",
            rows.len(),
            skill.id
        );
        if text.chars().count() + row.chars().count() + receipt.chars().count() > CATALOG_BUDGET {
            break;
        }
        text.push_str(&row);
        shown += 1;
        last = &skill.id;
    }
    if shown < rows.len() {
        text.push_str(&format!(
            "其余 {} 条；下一页 after={last}\n",
            rows.len() - shown
        ));
    }
    text.push_str("</skills>");
    Some(text)
}

fn instruction_block(skill: &SkillDef) -> anyhow::Result<String> {
    let content = read_content(skill)?;
    let resources = if skill.embedded_content.is_some() {
        "打包资源，无本地资料目录".into()
    } else {
        format!(
            "相对资料路径基于 {}",
            skill
                .path
                .parent()
                .ok_or_else(|| anyhow::anyhow!("Skill 缺少父目录"))?
                .display()
        )
    };
    Ok(format!(
        "Skill {}\n来源: {}\n版本: {}\n{}\n{}",
        skill.id,
        skill.path.display(),
        skill.revision,
        resources,
        crate::markdown::parse_frontmatter(content.trim_start_matches('\u{feff}')).body
    ))
}

/// Explicit references in this run's original task input are assembled apart
/// from agent/system and never added to persisted conversation messages.
#[derive(Clone)]
pub struct InvokedSkills(String);

impl std::fmt::Debug for InvokedSkills {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InvokedSkills")
            .field("chars", &self.0.chars().count())
            .finish()
    }
}

impl InvokedSkills {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Resolve only the input actually admitted for this run, before attachment
/// expansion. Source references preserve that input's authority; they do not
/// prove human authorship or grant additional permission.
pub fn invoked_context(
    snapshot: &crate::HarnessSnapshot,
    prompt: &str,
) -> anyhow::Result<Option<InvokedSkills>> {
    if snapshot.profile() == crate::ProfileKind::Isolated
        || !snapshot
            .materialize_tools()
            .iter()
            .any(|tool| tool.name() == "skill_read")
    {
        return Ok(None);
    }
    Ok(invoked_block(prompt)?.map(InvokedSkills))
}

fn invoked_block(prompt: &str) -> anyhow::Result<Option<String>> {
    if !prompt.contains('$') {
        return Ok(None);
    }
    let catalog = global_catalog()?;
    let prefs = preferences_at(&global_home()?)?;
    invoked_block_at(prompt, &catalog, &prefs)
}

fn invoked_block_at(
    prompt: &str,
    catalog: &[SkillDef],
    prefs: &SkillPreferences,
) -> anyhow::Result<Option<String>> {
    let mut invoked = BTreeSet::new();
    let mut text = String::new();
    let mut fence: Option<char> = None;
    for line in prompt.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            let marker = trimmed.chars().next().unwrap();
            if fence == Some(marker) {
                fence = None;
            } else if fence.is_none() {
                fence = Some(marker);
            }
            continue;
        }
        if fence.is_some() || trimmed.starts_with('>') {
            continue;
        }
        for (start, _) in line.match_indices('$') {
            if line[..start]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                continue;
            }
            let selector: String = line[start + 1..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | ':'))
                .collect();
            if !catalog
                .iter()
                .any(|skill| skill.id == selector || skill.name == selector)
            {
                continue;
            }
            let skill = resolve(catalog, &selector)?;
            if !invoked.insert(skill.id.clone()) {
                continue;
            }
            anyhow::ensure!(
                is_enabled(skill, prefs),
                "Skill 已停用：{}，请在 Skills 中启用",
                skill.id
            );
            anyhow::ensure!(skill.user_invocable, "Skill 不允许手动调用：{}", skill.id);
            if text.is_empty() {
                text.push_str("<invoked-skills>\n本轮输入引用的技能正文快照，遵循当前用户指令；技能本身不增加操作授权。\n");
            }
            text.push_str(&instruction_block(skill)?);
            text.push('\n');
        }
    }
    if text.is_empty() {
        Ok(None)
    } else {
        text.push_str("</invoked-skills>");
        Ok(Some(text))
    }
}

pub struct SkillListTool;

#[async_trait::async_trait]
impl crate::Tool for SkillListTool {
    fn name(&self) -> &'static str {
        "skill_list"
    }
    fn description(&self) -> String {
        "List or search available skill metadata; continue after the last returned id.".into()
    }
    fn read_only_support(&self) -> crate::tool::ReadOnlySupport {
        crate::tool::ReadOnlySupport::ReadOnly
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({"type":"object","properties":{"query":{"type":"string"},"after":{"type":"string"}},"additionalProperties":false})
    }
    fn concurrency(&self, _: &serde_json::Value, ctx: &crate::ToolCtx) -> crate::ToolConcurrency {
        crate::ToolConcurrency::shared_worktree(ctx)
    }
    async fn execute(&self, input: serde_json::Value, _: &crate::ToolCtx) -> crate::ToolOutput {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Input {
            #[serde(default)]
            query: String,
            after: Option<String>,
        }
        let result = (|| -> anyhow::Result<String> {
            let input: Input = serde_json::from_value(input)?;
            Ok(
                catalog_block(&enabled_catalog()?, &input.query, input.after.as_deref())
                    .unwrap_or_else(|| "没有匹配的可自动选用技能。".into()),
            )
        })();
        match result {
            Ok(text) => crate::ToolOutput::ok(text),
            Err(error) => crate::ToolOutput::error(error.to_string()),
        }
    }
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
        let result = (|| -> anyhow::Result<crate::ToolOutput> {
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
            let output = crate::ToolOutput::ok(instruction_block(skill)?);
            Ok(if skill.embedded_content.is_none() {
                output.with_external_source(
                    crate::ExternalSourceKind::File,
                    skill.path.to_string_lossy(),
                )
            } else {
                output
            })
        })();
        match result {
            Ok(output) => output,
            Err(error) => crate::ToolOutput::error(error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str, body: &str) -> SkillDef {
        let content = format!("---\nname: {name}\ndescription: Description {name}\n---\n{body}");
        SkillDef {
            id: format!("personal:{name}"),
            source: "personal".into(),
            name: name.into(),
            description: format!("Description {name}"),
            path: format!("personal:{name}/SKILL.md").into(),
            revision: revision(&content),
            content_root: None,
            embedded_content: Some(content),
            disable_model_invocation: false,
            user_invocable: true,
        }
    }

    #[test]
    fn bounded_catalog_pages_keep_every_eligible_entry_discoverable() {
        assert!(catalog_block(&[], "", None).is_none());
        let mut skills: Vec<_> = (0..80)
            .map(|n| {
                let mut skill = fixture(&format!("skill-{n:03}"), "BODY_CANARY");
                skill.description = "多字节说明".repeat(100);
                skill
            })
            .collect();
        skills[1].disable_model_invocation = true;
        assert!(catalog_block(&[skills[1].clone()], "", None).is_none());
        let mut cursor = None;
        let mut seen = BTreeSet::new();
        loop {
            let page = catalog_block(&skills, "", cursor.as_deref()).unwrap();
            assert!(page.chars().count() <= CATALOG_BUDGET);
            assert!(!page.contains("BODY_CANARY"));
            let ids: Vec<_> = page
                .lines()
                .filter_map(|line| line.strip_prefix("- "))
                .map(|line| line.split_once(": ").unwrap().0.to_string())
                .collect();
            assert!(!ids.is_empty());
            for id in &ids {
                assert!(seen.insert(id.clone()));
            }
            if !page.contains("下一页") {
                break;
            }
            cursor = ids.last().cloned();
        }
        assert_eq!(seen.len(), 79);
        assert!(!seen.contains("personal:skill-001"));
        let page = catalog_block(&skills, "skill-079", None).unwrap();
        assert!(page.contains("personal:skill-079"));
        assert!(!page.contains("personal:skill-000"));
        let original = catalog_block(&skills, "", None).unwrap();
        skills.last_mut().unwrap().revision = revision("changed source");
        assert_ne!(original, catalog_block(&skills, "", None).unwrap());
    }

    #[test]
    fn explicit_references_obey_independent_invocation_controls_and_source_versions() {
        let mut skill = fixture("sample-check", "BODY_CANARY");
        skill.disable_model_invocation = true;
        let prefs = SkillPreferences::default();
        let skills = vec![skill.clone()];
        let block = invoked_block_at(
            "请使用$sample-check 和 $personal:sample-check",
            &skills,
            &prefs,
        )
        .unwrap()
        .unwrap();
        assert_eq!(block.matches("BODY_CANARY").count(), 1);
        assert!(block.contains(&skill.revision));
        assert!(!block.contains("description:"));
        assert!(invoked_block_at(
            "```powershell\n$sample-check\n```\n> $sample-check\n$UNKNOWN $sample-check-other",
            &skills,
            &prefs
        )
        .unwrap()
        .is_none());
        let mut prefs = prefs;
        prefs.disabled.insert(skill.id.clone());
        assert!(invoked_block_at("$sample-check", &skills, &prefs).is_err());
        skill.user_invocable = false;
        assert!(invoked_block_at(
            "$sample-check",
            &[skill.clone()],
            &SkillPreferences::default()
        )
        .is_err());
        skill.user_invocable = true;
        skill.embedded_content = Some("changed source".into());
        assert!(invoked_block_at("$sample-check", &[skill], &SkillPreferences::default()).is_err());
    }

    #[test]
    fn invocation_requires_installed_capability() {
        let harness = crate::Harness::default();
        let snapshot = harness
            .resolve(&crate::ResolveCtx {
                profile: crate::ProfileKind::Isolated,
                cwd: ".".into(),
                project_root: ".".into(),
                config: std::sync::Arc::new(crate::KanzeiConfig::default()),
            })
            .unwrap();
        assert!(invoked_context(&snapshot, "$builtin:pdf")
            .unwrap()
            .is_none());
    }

    #[test]
    fn global_skill_sources_refresh_metadata_and_keep_invocation_separate() {
        kanzei_base::test_support::isolated_test(
            module_path!(),
            "global_skill_sources_refresh_metadata_and_keep_invocation_separate",
            || {
                let home =
                    std::env::temp_dir().join(format!("kz-skills-context-{}", std::process::id()));
                std::fs::create_dir_all(home.join("skills/sample")).unwrap();
                std::env::set_var("KANZEI_HOME", &home);
                let path = home.join("skills/sample/SKILL.md");
                std::fs::write(
                    &path,
                    "---\nname: sample\ndescription: INITIAL_DESCRIPTION\n---\nBODY_CANARY",
                )
                .unwrap();
                let mut harness = crate::Harness::default();
                harness.add(crate::MarkdownComponent);
                let snapshot = harness
                    .resolve(&crate::ResolveCtx {
                        profile: crate::ProfileKind::DevAuto,
                        cwd: home.clone(),
                        project_root: home.clone(),
                        config: std::sync::Arc::new(crate::KanzeiConfig::default()),
                    })
                    .unwrap();
                let invoked = invoked_context(&snapshot, "$sample").unwrap().unwrap();
                assert!(invoked.as_str().contains("BODY_CANARY"));
                let (stable, _) = snapshot.stable_system_baseline_with_report().unwrap();
                assert!(!stable.contains("BODY_CANARY"));
                let (catalog, report) = snapshot.refreshable_system_baseline_with_report().unwrap();
                assert!(catalog.contains("INITIAL_DESCRIPTION"));
                assert!(!catalog.contains("BODY_CANARY"));
                assert!(report
                    .iter()
                    .any(|source| source.key == "core/skills" && source.refresh_each_step));
                std::fs::write(
                    &path,
                    "---\nname: sample\ndescription: UPDATED_DESCRIPTION\n---\nNEW_BODY",
                )
                .unwrap();
                let (catalog, _) = snapshot.refreshable_system_baseline_with_report().unwrap();
                assert!(catalog.contains("UPDATED_DESCRIPTION"));
                assert!(invoked.as_str().contains("BODY_CANARY"));
                assert!(!invoked.as_str().contains("NEW_BODY"));
                set_enabled_at(&home, "personal:sample", false).unwrap();
                let (catalog, _) = snapshot.refreshable_system_baseline_with_report().unwrap();
                assert!(!catalog.contains("personal:sample"));
                std::fs::write(home.join("skills.json"), "invalid JSON").unwrap();
                assert!(snapshot.refreshable_system_baseline_with_report().is_err());
                std::fs::remove_dir_all(home).unwrap();
            },
        );
    }

    #[test]
    fn skill_catalog_cannot_read_metadata_through_a_directory_link_outside_its_root() {
        let root = std::env::temp_dir().join(format!(
            "kz-skill-boundary-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let home = root.join("home");
        let outside = root.join("outside");
        std::fs::create_dir_all(home.join("skills")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(
            outside.join("SKILL.md"),
            "---\nname: leak\ndescription: PRIVATE_DESCRIPTION_CANARY\n---\nPRIVATE_BODY_CANARY",
        )
        .unwrap();
        let link = home.join("skills").join("leak");
        #[cfg(windows)]
        assert!(std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .output()
            .unwrap()
            .status
            .success());
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        let catalog = catalog_at(&home, None).unwrap();
        assert!(
            !catalog
                .iter()
                .any(|skill| skill.description.contains("PRIVATE_DESCRIPTION_CANARY")),
            "catalog metadata reads must stay inside the granted skills directory"
        );
        assert!(resolve(&catalog, "personal:leak").is_err());
        let good = home.join("skills").join("good");
        std::fs::create_dir_all(&good).unwrap();
        std::fs::write(
            good.join("SKILL.md"),
            "---\nname: good\ndescription: Owned source\n---\nORIGINAL_SKILL_BODY",
        )
        .unwrap();
        let catalog = catalog_at(&home, None).unwrap();
        let skill = resolve(&catalog, "personal:good").unwrap();
        assert!(read_content(skill).unwrap().contains("ORIGINAL_SKILL_BODY"));
        let backup = root.join("good-before-link");
        assert!(good.starts_with(&root) && backup.starts_with(&root));
        std::fs::rename(&good, backup).unwrap();
        #[cfg(windows)]
        assert!(std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&good)
            .arg(&outside)
            .output()
            .unwrap()
            .status
            .success());
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &good).unwrap();
        assert!(
            read_content(skill).is_err(),
            "retargeting a catalog entry must not retarget its body read"
        );
    }
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
        set_enabled_at(&home, "personal:pdf", false).unwrap();
        set_enabled_at(&home, "builtin:pdf", true).unwrap();
        let prefs = preferences_at(&home).unwrap();
        assert!(is_enabled(resolve(&skills, "builtin:pdf").unwrap(), &prefs));
        assert!(!is_enabled(
            resolve(&skills, "personal:pdf").unwrap(),
            &prefs
        ));
        assert!(!home.join("builtin-skills").exists());
        std::fs::write(home.join("skills.json"), r#"{"disabled":["pdf"]}"#).unwrap();
        assert!(preferences_at(&home).is_err());
        assert!(set_enabled_at(&home, "builtin:pdf", false).is_err());
        std::fs::remove_dir_all(home).unwrap();
    }
}
