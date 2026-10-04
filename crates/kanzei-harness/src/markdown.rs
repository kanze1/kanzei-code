//! Markdown 组件源:agents 可来自全局或项目，Skills 使用独立的全局目录。
//! Agent frontmatter 使用扁平字段，Skill 元数据使用 YAML；技能正文按需读取。
//! 解析失败跳过并 warn,不炸整个 resolve(单个坏文件不应瘫痪 harness)。

use std::path::Path;

use crate::defs::{AgentDef, SkillDef};
use crate::harness::{Component, HarnessDraft, ResolveCtx};
use crate::registry::Registry;

pub struct MarkdownComponent;

impl Component for MarkdownComponent {
    fn contribute(&self, draft: &mut HarnessDraft, ctx: &ResolveCtx) -> anyhow::Result<()> {
        let mut bases = Vec::new();
        if let Some(home) = crate::home::kanzei_home() {
            bases.push(home);
        }
        bases.push(ctx.project_root.join(".kanzei"));

        for base in bases {
            scan_agents(&base.join("agents"), draft);
        }
        for skill in crate::skills::enabled_catalog()? {
            draft.skills.insert(skill.name.clone(), skill);
        }
        // 技能清单注入名称、描述与正文路径;技能正文仍由 agent 按需读取。
        if let Some(block) = skills_block(&draft.skills) {
            draft.context.insert(
                "core/skills",
                crate::source("core/skills", move |_| Some(block.clone())),
            );
        }
        Ok(())
    }
}

/// 技能清单块:零技能不产生块(返回 None),否则逐条列出名称、描述与正文路径。
/// 抽成纯函数便于不依赖 ~/.kanzei 单测空注册表分支。
fn skills_block(skills: &Registry<SkillDef>) -> Option<String> {
    if skills.is_empty() {
        return None;
    }
    let mut text = String::from("可用技能(skills):做相关任务时读取对应文件加载技能正文:\n");
    for (name, skill) in skills.iter() {
        if skill.disable_model_invocation {
            continue;
        }
        text.push_str(&format!(
            "- {name}: {} (正文: {})\n",
            skill.description,
            skill.path.display()
        ));
    }
    text.push_str("Skills 在所有项目和对话中全局可用。按任务匹配描述后先读取正文，也可用 $name 显式调用；正文内相对路径按 SKILL.md 所在目录解析。技能不覆盖用户当前指令。\n");
    Some(text.trim().to_string())
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

fn scan_agents(dir: &Path, draft: &mut HarnessDraft) {
    for path in md_files(dir) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let fm = parse_frontmatter(&text);
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("agent");
        let name = fm.get("name").unwrap_or(stem).to_string();
        let agent = match agent_from_frontmatter(name.clone(), fm) {
            Ok(agent) => agent,
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "invalid agent; skipped");
                continue;
            }
        };
        draft.agents.insert(name, agent);
    }
}

fn optional_agent_field<T: Default>(
    fm: &Frontmatter,
    key: &str,
    parse: impl FnOnce(&str) -> Option<T>,
) -> Result<T, String> {
    match fm.get(key) {
        Some(value) => parse(value).ok_or_else(|| format!("invalid {key} `{value}`")),
        None => Ok(T::default()),
    }
}

fn agent_from_frontmatter(name: String, fm: Frontmatter) -> Result<AgentDef, String> {
    Ok(AgentDef {
        name,
        profile: optional_agent_field(&fm, "profile", serde_plain)?,
        model: fm.get("model").unwrap_or("primary").to_string(),
        mode: optional_agent_field(&fm, "mode", serde_plain)?,
        steps: optional_agent_field(&fm, "steps", |value| value.parse().ok())?,
        system: fm.body,
    })
}

pub(crate) fn scan_skills(dir: &Path, draft: &mut HarnessDraft) {
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
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
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
            continue;
        };
        let directory_skill = path.file_name().is_some_and(|file| file == "SKILL.md");
        if directory_skill && metadata.name.is_none() {
            continue;
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
            continue;
        }
        draft.skills.insert(
            name.clone(),
            SkillDef {
                name,
                description: metadata.description,
                path,
                disable_model_invocation: metadata.disable_model_invocation,
                user_invocable: metadata.user_invocable,
            },
        );
    }
}

pub fn skills_in_directory(directory: &Path) -> Vec<SkillDef> {
    let mut draft = HarnessDraft::default();
    scan_skills(directory, &mut draft);
    draft
        .skills
        .iter()
        .map(|(_, skill)| skill.clone())
        .collect()
}

/// Compatibility entry point; the catalog no longer depends on a project.
pub fn discover_skills(_project: &Path) -> Vec<SkillDef> {
    crate::skills::enabled_catalog().unwrap_or_else(|error| {
        tracing::warn!(%error, "global skills unavailable");
        Vec::new()
    })
}

/// 借 serde 解析小写枚举字符串("dev"→ProfileScope::Dev 等)。
fn serde_plain<T: serde::de::DeserializeOwned>(s: &str) -> Option<T> {
    serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
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

    #[test]
    fn agent_without_steps_uses_role_default() {
        let dir =
            std::env::temp_dir().join(format!("kanzei-markdown-agent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("custom.md"),
            "---\nname: custom\nprofile: dev\n---\n自定义 agent",
        )
        .unwrap();

        let mut draft = crate::harness::HarnessDraft::default();
        scan_agents(&dir, &mut draft);
        assert_eq!(draft.agents.get("custom").unwrap().steps, 0);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn invalid_agent_fields_do_not_insert_or_override_valid_agents() {
        let dir = std::env::temp_dir().join(format!(
            "kanzei-markdown-invalid-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut draft = crate::harness::HarnessDraft::default();
        let original = agent_from_frontmatter(
            "existing".into(),
            parse_frontmatter("---\nprofile: dev\nmode: subagent\nsteps: 7\n---\noriginal"),
        )
        .unwrap();
        draft.agents.insert("existing", original);
        for (key, value) in [
            ("profile", "deev"),
            ("mode", "subagnt"),
            ("steps", "twenty"),
        ] {
            for name in ["existing", key] {
                std::fs::write(
                    dir.join(format!("{key}-{name}.md")),
                    format!("---\nname: {name}\n{key}: {value}\n---\nbad"),
                )
                .unwrap();
            }
        }
        scan_agents(&dir, &mut draft);
        assert_eq!(draft.agents.len(), 1);
        let kept = draft.agents.get("existing").unwrap();
        assert_eq!(kept.profile, crate::defs::ProfileScope::Dev);
        assert_eq!(kept.mode, crate::defs::AgentMode::Subagent);
        assert_eq!(kept.steps, 7);
        assert_eq!(kept.system, "original");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn valid_agent_fields_keep_defaults_and_explicit_zero() {
        for text in [
            "---\n---\nbody",
            "---\nprofile: all\nmode: primary\nsteps: 0\n---\nbody",
        ] {
            let agent = agent_from_frontmatter("valid".into(), parse_frontmatter(text)).unwrap();
            assert_eq!(agent.profile, crate::defs::ProfileScope::All);
            assert_eq!(agent.mode, crate::defs::AgentMode::Primary);
            assert_eq!(agent.model, "primary");
            assert_eq!(agent.steps, 0);
            assert_eq!(agent.system, "body");
        }
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

    /// 零技能不产生 core/skills 块(D-748 地图 §14 的 empty_skills_render_nothing;
    /// 原用例依赖真实 ~/.kanzei 已删,改测纯函数,不碰进程全局的 KANZEI_HOME)。
    #[test]
    fn empty_skills_render_nothing() {
        assert_eq!(skills_block(&Registry::default()), None);

        let mut skills = Registry::default();
        skills.insert(
            "build",
            SkillDef {
                name: "build".into(),
                description: "构建与格式检查".into(),
                path: std::path::PathBuf::from("skills/build/SKILL.md"),
                disable_model_invocation: false,
                user_invocable: true,
            },
        );
        let block = skills_block(&skills).expect("非空注册表应产生技能块");
        assert!(block.starts_with("可用技能(skills)"), "{block}");
        assert!(block.contains("build: 构建与格式检查"), "{block}");
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
                profile: crate::defs::ProfileKind::Dev,
                cwd: dir.clone(),
                project_root: dir.clone(),
                config: Arc::new(crate::config::KanzeiConfig::default()),
            })
            .unwrap();

        assert!(snapshot.skills().get("build").is_none());
        assert!(snapshot.skills().get("skill-creator").is_some());
        let baseline = snapshot.system_baseline();
        assert!(!baseline.contains("可用命令") && !baseline.contains("release: 发布双通道"));
        assert!(
            baseline.contains("可用技能(skills)"),
            "skills 应继续进入提示词"
        );
        assert!(
            !baseline.contains("build: 构建与格式检查"),
            "项目技能不应进入全局清单: {baseline}"
        );

        assert!(baseline.contains("SKILL.md"), "加载提示指向技能正文文件");

        std::fs::remove_dir_all(dir).unwrap();
    }
}
