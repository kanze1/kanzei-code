//! markdown 组件源:扫描 ~/.kanzei/ 与项目 .kanzei/ 下的 agents/commands/skills 目录。
//! frontmatter 为 `---` 包围的扁平 `key: value`;正文即 system/template。
//! 解析失败跳过并 warn,不炸整个 resolve(单个坏文件不应瘫痪 harness)。

use std::path::Path;

use crate::defs::{AgentDef, CommandDef, SkillDef, DEFAULT_AGENT_STEPS};
use crate::harness::{Component, HarnessDraft, ResolveCtx};

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
            scan_commands(&base.join("commands"), draft);
            scan_skills(&base.join("skills"), draft);
        }
        // D-184:commands/skills 消费端——渲染进 system baseline。
        // 扫描结果即 final(contribute 内已填充),渲染成静态文本克隆进闭包
        // (ContextSource 闭包只拿 ResolveCtx,不持有 draft)。
        // commands → 可调用清单;skills → 加载提示(正文留在文件,按需 read)。
        let mut blocks = Vec::new();
        if !draft.commands.is_empty() {
            let mut text = String::from(
                "可用命令(commands):按名调用,模板正文在对应 md 文件,参数用 $ARGUMENTS / $1..$N:\n",
            );
            for (name, cmd) in draft.commands.iter() {
                text.push_str(&format!("- {name}: {}\n", cmd.description));
                if let Some(agent) = &cmd.agent {
                    text.push_str(&format!("  (限定 agent: {agent})\n"));
                }
            }
            blocks.push(text.trim().to_string());
        }
        if !draft.skills.is_empty() {
            let mut text = String::from("可用技能(skills):做相关任务时读取对应文件加载技能正文:\n");
            for (name, skill) in draft.skills.iter() {
                text.push_str(&format!(
                    "- {name}: {} (正文: {})\n",
                    skill.description,
                    skill.path.display()
                ));
            }
            blocks.push(text.trim().to_string());
        }
        if !blocks.is_empty() {
            let block = blocks.join("\n\n");
            draft.context.insert(
                "core/commands_skills",
                crate::source("core/commands_skills", move |_| Some(block.clone())),
            );
        }
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

fn scan_agents(dir: &Path, draft: &mut HarnessDraft) {
    for path in md_files(dir) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let fm = parse_frontmatter(&text);
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("agent");
        let name = fm.get("name").unwrap_or(stem).to_string();
        let agent = AgentDef {
            name: name.clone(),
            profile: fm.get("profile").and_then(serde_plain).unwrap_or_default(),
            model: fm.get("model").unwrap_or("primary").to_string(),
            mode: fm.get("mode").and_then(serde_plain).unwrap_or_default(),
            steps: fm
                .get("steps")
                .and_then(|s| s.parse().ok())
                .unwrap_or(DEFAULT_AGENT_STEPS),
            system: fm.body,
        };
        draft.agents.insert(name, agent);
    }
}

fn scan_commands(dir: &Path, draft: &mut HarnessDraft) {
    for path in md_files(dir) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let fm = parse_frontmatter(&text);
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("command");
        let name = fm.get("name").unwrap_or(stem).to_string();
        draft.commands.insert(
            name.clone(),
            CommandDef {
                name,
                description: fm.get("description").unwrap_or("").to_string(),
                agent: fm.get("agent").map(str::to_string),
                template: fm.body,
            },
        );
    }
}

fn scan_skills(dir: &Path, draft: &mut HarnessDraft) {
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
    for path in candidates {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let fm = parse_frontmatter(&text);
        let stem = if path.file_name().map(|f| f == "SKILL.md").unwrap_or(false) {
            path.parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .unwrap_or("skill")
        } else {
            path.file_stem().and_then(|s| s.to_str()).unwrap_or("skill")
        };
        let name = fm.get("name").unwrap_or(stem).to_string();
        let Some(description) = fm.get("description").map(str::to_string) else {
            tracing::warn!(path = %path.display(), "skill missing description; skipped");
            continue;
        };
        draft.skills.insert(
            name.clone(),
            SkillDef {
                name,
                description,
                path,
            },
        );
    }
}

/// 借 serde 解析小写枚举字符串("dev"→ProfileScope::Dev 等)。
fn serde_plain<T: serde::de::DeserializeOwned>(s: &str) -> Option<T> {
    serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
}

#[cfg(test)]
mod tests {
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
    fn agent_without_steps_uses_finite_default() {
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
        assert_eq!(
            draft.agents.get("custom").unwrap().steps,
            DEFAULT_AGENT_STEPS
        );

        std::fs::remove_dir_all(dir).unwrap();
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

    /// D-184:commands/skills 解析后必须被消费——渲染进 system baseline。
    /// 放命令与技能文件,resolve 后 stable baseline 含命令名/描述与技能名/加载提示,
    /// 不再是「解析了但没人读」的注册表。
    #[test]
    fn commands_and_skills_render_into_system_baseline() {
        let dir =
            std::env::temp_dir().join(format!("kanzei-markdown-consume-{}", std::process::id()));
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
            "---\ndescription: 构建与格式检查\n---\n构建技能正文",
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

        // commands/skills 文件存在 → 注册表有货,且进了 stable baseline。
        assert_eq!(snapshot.commands().len(), 1);
        assert_eq!(snapshot.skills().len(), 1);
        let baseline = snapshot.system_baseline();
        assert!(
            baseline.contains("可用命令(commands)"),
            "commands 应进提示词: {baseline}"
        );
        assert!(baseline.contains("release: 发布双通道"), "命令清单含描述");
        assert!(baseline.contains("可用技能(skills)"), "skills 应进提示词");
        assert!(
            baseline.contains("build: 构建与格式检查"),
            "技能清单含描述: {baseline}"
        );
        assert!(baseline.contains("SKILL.md"), "加载提示指向技能正文文件");

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// D-184:无命令/技能文件时 baseline 不产生空块(零内容不占上下文)。
    #[test]
    fn empty_commands_skills_render_nothing() {
        let dir =
            std::env::temp_dir().join(format!("kanzei-markdown-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

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

        let baseline = snapshot.system_baseline();
        assert!(
            !baseline.contains("可用命令") && !baseline.contains("可用技能"),
            "空注册表不应渲染: {baseline:?}"
        );

        std::fs::remove_dir_all(dir).unwrap();
    }
}
