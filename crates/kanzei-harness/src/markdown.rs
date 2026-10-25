//! Markdown 组件源:agents 可来自全局或项目，Skills 使用独立的全局目录。
//! Agent 来源解析和诊断由 agent_sources 统一拥有；技能正文按需读取。

use std::path::Path;

use crate::defs::SkillDef;
use crate::harness::{Component, HarnessDraft, ResolveCtx};

pub struct MarkdownComponent;

impl Component for MarkdownComponent {
    fn contribute(&self, draft: &mut HarnessDraft, ctx: &ResolveCtx) -> anyhow::Result<()> {
        let root = (!crate::is_general_conversation_root(&ctx.project_root))
            .then_some(ctx.project_root.as_path());
        let agents = crate::agent_sources::load_at(root, crate::kanzei_home().as_deref())?;
        crate::agent_sources::register(agents, draft);
        for skill in crate::skills::enabled_catalog()? {
            let path = skill.path.clone();
            draft.skills.insert_at(skill.id.clone(), skill, path);
        }
        for (name, tool) in [
            (
                "skill_read",
                std::sync::Arc::new(crate::skills::SkillReadTool)
                    as std::sync::Arc<dyn crate::Tool>,
            ),
            (
                "skill_list",
                std::sync::Arc::new(crate::skills::SkillListTool)
                    as std::sync::Arc<dyn crate::Tool>,
            ),
        ] {
            draft.tools.insert(name, tool);
            draft.deferred_tools.insert(name.into());
            draft.permissions.push(crate::Rule {
                action: name.into(),
                resource: "*".into(),
                effect: crate::Effect::Allow,
            });
        }
        // Refresh catalog metadata; bodies enter context only when selected.
        draft.context.insert(
            "core/skills",
            crate::refreshing_fallible_source(
                "core/skills",
                crate::ContextRequirement::Required,
                |_| {
                    Ok(crate::skills::catalog_block(
                        &crate::skills::enabled_catalog()?,
                        "",
                        None,
                    ))
                },
            ),
        );
        Ok(())
    }
}

pub struct Frontmatter {
    pub pairs: Vec<(String, String)>,
    pub body: String,
}

impl Frontmatter {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.pairs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// `---` 包围的扁平 key: value;无 frontmatter 时 pairs 为空、全文为 body。
pub fn parse_frontmatter(text: &str) -> Frontmatter {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Frontmatter {
            pairs: Vec::new(),
            body: text.to_string(),
        };
    }
    let mut pairs = Vec::new();
    let mut body = String::new();
    // 用剩余文本的真实字节切分,不靠 lines() 重建偏移:lines() 会剥掉 `\r\n` 两个字节,
    // 而按 len()+1 累加每行只算一个,CRLF 文件会逐行欠 1 字节,收尾定位落进分隔符甚至
    // 上一行;若落点切在多字节字符中间,body 会整个变空(Windows 上必现,D-052)。
    let mut rest = match text.split_once('\n') {
        Some((_, rest)) => rest,
        None => "",
    };
    loop {
        let (line, tail) = match rest.split_once('\n') {
            Some((line, tail)) => (line, tail),
            None => (rest, ""),
        };
        if line.trim() == "---" {
            body = tail.trim().to_string();
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            pairs.push((key.trim().to_string(), value.trim().to_string()));
        }
        if tail.is_empty() {
            break;
        }
        rest = tail;
    }
    Frontmatter { pairs, body }
}

fn md_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<_> = read
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e == "md").unwrap_or(false))
        .collect();
    files.sort();
    files
}

fn collect_skills(dir: &Path) -> Vec<SkillDef> {
    let mut skills = Vec::new();
    // 两种布局:skills/<name>/SKILL.md 或 skills/<name>.md
    let mut candidates = md_files(dir);
    if let Ok(read) = std::fs::read_dir(dir) {
        for entry in read.filter_map(|e| e.ok()) {
            let skill_md = entry.path().join("SKILL.md");
            if skill_md.is_file() {
                candidates.push(skill_md);
            }
        }
    }
    candidates.sort();
    for path in candidates {
        let text = match crate::skills::read_skill_file(dir, &path) {
            Ok(text) => text,
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "skill source rejected");
                continue;
            }
        };
        if let Some(skill) = parse_skill(dir, path, &text) {
            skills.push(skill);
        }
    }
    skills
}

fn parse_skill(dir: &Path, path: std::path::PathBuf, text: &str) -> Option<SkillDef> {
    let stem = if path.file_name().map(|f| f == "SKILL.md").unwrap_or(false) {
        path.parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("skill")
    } else {
        path.file_stem().and_then(|s| s.to_str()).unwrap_or("skill")
    };
    #[derive(serde::Deserialize)]
    struct Metadata {
        name: Option<String>,
        description: String,
        #[serde(default, rename = "disable-model-invocation")]
        disable_model_invocation: bool,
        #[serde(default = "yes", rename = "user-invocable")]
        user_invocable: bool,
    }
    fn yes() -> bool {
        true
    }
    let normalized = text.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let metadata = normalized.strip_prefix("---\n").and_then(|body| {
        let end = body.lines().position(|line| line == "---")?;
        let yaml = body.lines().take(end).collect::<Vec<_>>().join("\n");
        serde_yaml_ng::from_str::<Metadata>(&yaml).ok()
    });
    let Some(metadata) = metadata else {
        tracing::warn!(path = %path.display(), "skill missing description; skipped");
        return None;
    };
    let directory_skill = path.file_name().is_some_and(|file| file == "SKILL.md");
    if directory_skill && metadata.name.is_none() {
        return None;
    }
    let name = metadata.name.unwrap_or_else(|| stem.to_string());
    if directory_skill && name != stem
        || name.is_empty()
        || name.chars().count() > 64
        || name.starts_with('-')
        || name.ends_with('-')
        || name.contains("--")
        || !name
            .chars()
            .all(|c| c == '-' || c.is_alphanumeric() && !c.is_uppercase())
        || metadata.description.trim().is_empty()
        || metadata.description.chars().count() > 1024
    {
        return None;
    }
    Some(SkillDef {
        id: name.clone(),
        source: String::new(),
        embedded_content: None,
        name,
        description: metadata.description,
        path,
        revision: crate::skills::revision(text),
        content_root: Some(dir.to_path_buf()),
        disable_model_invocation: metadata.disable_model_invocation,
        user_invocable: metadata.user_invocable,
    })
}

/// Read one explicitly selected directory without scanning its parent or siblings.
pub fn skill_in_directory(directory: &Path) -> anyhow::Result<SkillDef> {
    let path = directory.join("SKILL.md");
    let text = crate::skills::read_skill_file(directory, &path)?;
    parse_skill(directory, path, &text)
        .ok_or_else(|| anyhow::anyhow!("SKILL.md 无效，请检查名称、描述和 YAML"))
}

pub fn skills_in_directory(directory: &Path) -> Vec<SkillDef> {
    collect_skills(directory)
}

#[cfg(test)]
pub(crate) fn scan_skills(dir: &Path, draft: &mut HarnessDraft) {
    for skill in collect_skills(dir) {
        draft.skills.insert(skill.name.clone(), skill);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn agent_skills_yaml_blocks_manual_flags_and_global_precedence() {
        let root = std::env::temp_dir().join(format!(
            "kz-agent-skills-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        for (base, description) in [
            (".codex", "legacy definition"),
            (".agents", "modern definition"),
        ] {
            let directory = root.join(base).join("skills/protocol-check");
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(directory.join("SKILL.md"), format!("---\nname: protocol-check\ndescription: >\n  {description}\n  with multiple lines\ndisable-model-invocation: true\nuser-invocable: false\nallowed-tools: [Read, Bash]\n---\nUse references/details.md")).unwrap();
        }
        let catalog = crate::skills::catalog_at(&root.join("global"), Some(&root)).unwrap();
        let skill = catalog
            .iter()
            .find(|skill| skill.name == "protocol-check")
            .unwrap();
        assert!(skill
            .description
            .contains("modern definition with multiple lines"));
        assert!(skill.disable_model_invocation);
        assert!(!skill.user_invocable);
        assert!(skill.path.starts_with(root.join(".agents")));
        let mut draft = crate::harness::HarnessDraft::default();
        let bad = root.join("bad");
        std::fs::create_dir_all(&bad).unwrap();
        std::fs::write(
            bad.join("malformed.md"),
            "---\nname: wrong\ndescription: [\n---\nignored",
        )
        .unwrap();
        super::scan_skills(&bad, &mut draft);
        assert!(draft.skills.is_empty());
        for name in ["分析", &"é".repeat(64)] {
            let directory = bad.join(name);
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(
                directory.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: Unicode skill\n---\nBody"),
            )
            .unwrap();
        }
        for (directory, metadata) in [
            ("missing-name", "description: No name".to_string()),
            (
                "wrong-directory",
                "name: other\ndescription: Wrong directory".to_string(),
            ),
            (
                "Éclair",
                "name: Éclair\ndescription: Uppercase name".to_string(),
            ),
            (
                &"é".repeat(65),
                format!("name: {}\ndescription: Too long", "é".repeat(65)),
            ),
        ] {
            let directory = bad.join(directory);
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(
                directory.join("SKILL.md"),
                format!("---\n{metadata}\n---\nBody"),
            )
            .unwrap();
        }
        super::scan_skills(&bad, &mut draft);
        assert_eq!(draft.skills.iter().count(), 2);
        assert!(draft.skills.get("分析").is_some());
        assert!(draft.skills.get(&"é".repeat(64)).is_some());
        std::fs::remove_dir_all(root).unwrap();
    }
    use super::*;
    use std::sync::Arc;

    use crate::harness::Harness;

    #[test]
    fn frontmatter_parsing() {
        let fm = parse_frontmatter("---\nname: build\nsteps: 20\n---\n正文在此");
        assert_eq!(fm.get("name"), Some("build"));
        assert_eq!(fm.get("steps"), Some("20"));
        assert_eq!(fm.body, "正文在此");

        let no_fm = parse_frontmatter("没有 frontmatter 的正文");
        assert!(no_fm.pairs.is_empty());
        assert_eq!(no_fm.body, "没有 frontmatter 的正文");
    }

    /// Windows 上文件多为 CRLF;正文不得被分隔符残留污染,更不得整体丢失(D-052)。
    #[test]
    fn crlf_与_lf_解析结果一致() {
        for keys in 1..=6 {
            let mut lf = String::from("---\n");
            for i in 0..keys {
                lf.push_str(&format!("键{i}: 中文值{i}\n"));
            }
            lf.push_str("---\n正文第一行\n正文第二行");
            let crlf = lf.replace('\n', "\r\n");

            let a = parse_frontmatter(&lf);
            let b = parse_frontmatter(&crlf);
            assert_eq!(a.pairs.len(), keys, "LF keys={keys}");
            assert_eq!(b.pairs.len(), keys, "CRLF keys={keys}");
            assert_eq!(a.get("键0"), Some("中文值0"));
            assert_eq!(b.get("键0"), Some("中文值0"));
            assert_eq!(a.body, "正文第一行\n正文第二行", "LF body keys={keys}");
            assert_eq!(b.body, "正文第一行\r\n正文第二行", "CRLF body keys={keys}");
            assert!(!b.body.is_empty(), "CRLF body 不得为空 keys={keys}");
            assert!(
                !b.body.starts_with('-'),
                "CRLF body 不得残留分隔符 keys={keys}"
            );
        }
    }

    /// commands 目录即使存在也不扫描;skills 清单仍进入 system baseline。
    #[test]
    fn project_skills_are_ignored_while_global_skills_render_into_system_baseline() {
        let dir =
            std::env::temp_dir().join(format!("kanzei-markdown-skills-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".kanzei/commands")).unwrap();
        std::fs::create_dir_all(dir.join(".kanzei/skills/build/SKILL.md").parent().unwrap())
            .unwrap();
        std::fs::write(
            dir.join(".kanzei/commands/release.md"),
            "---\nname: release\ndescription: 发布双通道\n---\n执行 package.ps1 -Publish",
        )
        .unwrap();
        std::fs::write(
            dir.join(".kanzei/skills/build/SKILL.md"),
            "---\nname: build\ndescription: 构建与格式检查\n---\n构建技能正文",
        )
        .unwrap();

        let mut harness = Harness::default();
        harness.add(MarkdownComponent);
        let snapshot = harness
            .resolve(&crate::harness::ResolveCtx {
                profile: crate::defs::ProfileKind::DevAuto,
                cwd: dir.clone(),
                project_root: dir.clone(),
                config: Arc::new(crate::config::KanzeiConfig::default()),
            })
            .unwrap();

        assert!(snapshot.skills().get("build").is_none());
        assert!(snapshot.skills().get("builtin:skill-creator").is_some());
        let baseline = snapshot.system_baseline().unwrap();
        assert!(!baseline.contains("可用命令") && !baseline.contains("release: 发布双通道"));
        assert!(baseline.contains("<skills>"), "skills 应继续进入提示词");
        assert!(
            !baseline.contains("build: 构建与格式检查"),
            "项目技能不应进入全局清单: {baseline}"
        );

        assert!(baseline.contains("skill_read(id)"));
        assert!(!baseline.contains("构建技能正文"));

        std::fs::remove_dir_all(dir).unwrap();
    }
}
