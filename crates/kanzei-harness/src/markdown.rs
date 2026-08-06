//! markdown 组件源:扫描 ~/.kanzei/ 与项目 .kanzei/ 下的 agents/commands/skills 目录。
//! frontmatter 为 `---` 包围的扁平 `key: value`;正文即 system/template。
//! 解析失败跳过并 warn,不炸整个 resolve(单个坏文件不应瘫痪 harness)。

use std::path::Path;

use crate::defs::{AgentDef, CommandDef, SkillDef};
use crate::harness::{Component, HarnessDraft, ResolveCtx};

pub struct MarkdownComponent;

impl Component for MarkdownComponent {
    fn contribute(&self, draft: &mut HarnessDraft, ctx: &ResolveCtx) -> anyhow::Result<()> {
        let mut bases = Vec::new();
        if let Some(home) = dirs::home_dir() {
            bases.push(home.join(".kanzei"));
        }
        bases.push(ctx.project_root.join(".kanzei"));

        for base in bases {
            scan_agents(&base.join("agents"), draft);
            scan_commands(&base.join("commands"), draft);
            scan_skills(&base.join("skills"), draft);
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
    let mut body_start = 0usize;
    let mut offset = text.lines().next().map(|l| l.len() + 1).unwrap_or(0);
    for line in lines {
        let line_len = line.len() + 1;
        if line.trim() == "---" {
            body_start = offset + line_len;
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            pairs.push((key.trim().to_string(), value.trim().to_string()));
        }
        offset += line_len;
    }
    let body = text.get(body_start..).unwrap_or("").trim().to_string();
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
            profile: fm
                .get("profile")
                .and_then(|s| serde_plain(s))
                .unwrap_or_default(),
            model: fm.get("model").unwrap_or("primary").to_string(),
            mode: fm
                .get("mode")
                .and_then(|s| serde_plain(s))
                .unwrap_or_default(),
            steps: fm.get("steps").and_then(|s| s.parse().ok()).unwrap_or(40),
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
}
