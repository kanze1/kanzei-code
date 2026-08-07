//! Memory 系统(R-103/R-104):文件优先的分级记忆。
//! 真源是 markdown 文件(一条一文件+平铺 frontmatter),人可编辑、git 可恢复;
//! SQLite 只存可重建派生物(FTS 索引/hits)。设计基线 docs/design/memory-system.md。
//!
//! 分级:scope(Global=~/.kanzei/memory, Project=<root>/.kanzei/memory)
//!     × category(preference/habit/fact/sop;episode 走 state.db 不落文件)。

mod store;
mod tools;

pub use store::{AddOutcome, MemoryStore, SearchHit};
pub use tools::{MemoryNoteTool, MemorySearchTool, MemoryStatsTool};

use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryScope {
    Global,
    Project,
}

impl MemoryScope {
    pub fn prefix(self) -> &'static str {
        match self {
            MemoryScope::Global => "U",
            MemoryScope::Project => "M",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MemoryScope::Global => "global",
            MemoryScope::Project => "project",
        }
    }
}

/// 合法 category(episode 除外:它是 state.db 里的轮次日志,不是文件记忆)。
pub const CATEGORIES: &[&str] = &["preference", "habit", "fact", "sop"];
pub const STATUSES: &[&str] = &["active", "stale"];

/// global scope 根目录;KANZEI_HOME 供测试与多环境覆盖。
pub fn global_memory_root() -> Option<PathBuf> {
    let home = std::env::var("KANZEI_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".kanzei")))?;
    Some(home.join("memory"))
}

pub fn project_memory_root(project_root: &std::path::Path) -> PathBuf {
    project_root.join(".kanzei").join("memory")
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryEntry {
    pub id: String,
    pub scope: String,
    pub category: String,
    pub title: String,
    /// 检索与触发的钩子:什么时候该想起这条记忆。写入侧强制非空。
    pub description: String,
    pub status: String,
    pub created: String,
    pub updated: String,
    /// 溯源:user | migration | run:<session>/<step> 等。
    pub source: String,
    /// 未知 frontmatter 键原样保留(D-084 的教训:宽容读,不丢数据)。
    pub extras: Vec<(String, String)>,
    pub body: String,
}

impl MemoryEntry {
    /// 文件名:<id>-<创建时 slug>.md,slug 终身不改(id 才是主键)。
    pub fn file_stem(&self) -> String {
        let slug = slugify(&self.title);
        if slug.is_empty() {
            self.id.clone()
        } else {
            format!("{}-{}", self.id, slug)
        }
    }
}

/// 标题 → 文件名片段:保留字母数字与 CJK,其余折叠为 '-';上限 40 字符。
pub fn slugify(title: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in title.chars() {
        let keep = ch.is_alphanumeric();
        if keep {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
        if out.chars().count() >= 40 {
            break;
        }
    }
    out.trim_end_matches('-').to_string()
}

/// 平铺 frontmatter 解析:宽容——缺 `---`、缺键、未知键都不报错;
/// 结构约束由写入侧(store)强制。
pub fn parse_entry(text: &str) -> MemoryEntry {
    let mut entry = MemoryEntry {
        id: String::new(),
        scope: String::new(),
        category: String::new(),
        title: String::new(),
        description: String::new(),
        status: "active".into(),
        created: String::new(),
        updated: String::new(),
        source: String::new(),
        extras: Vec::new(),
        body: String::new(),
    };
    let mut lines = text.lines();
    let Some(first) = lines.next() else {
        return entry;
    };
    if first.trim() != "---" {
        entry.body = text.trim_matches('\n').to_string();
        return entry;
    }
    let mut body_lines: Vec<&str> = Vec::new();
    let mut in_front = true;
    for line in lines {
        if in_front {
            if line.trim() == "---" {
                in_front = false;
                continue;
            }
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            match key {
                "id" => entry.id = value.into(),
                "scope" => entry.scope = value.into(),
                "category" => entry.category = value.into(),
                "title" => entry.title = value.into(),
                "description" => entry.description = value.into(),
                "status" => entry.status = value.into(),
                "created" => entry.created = value.into(),
                "updated" => entry.updated = value.into(),
                "source" => entry.source = value.into(),
                other => entry.extras.push((other.into(), value.into())),
            }
        } else {
            body_lines.push(line);
        }
    }
    entry.body = body_lines.join("\n").trim_matches('\n').to_string();
    entry
}

pub fn render_entry(entry: &MemoryEntry) -> String {
    let mut out = String::from("---\n");
    out.push_str(&format!("id: {}\n", entry.id));
    out.push_str(&format!("scope: {}\n", entry.scope));
    out.push_str(&format!("category: {}\n", entry.category));
    out.push_str(&format!("title: {}\n", entry.title));
    out.push_str(&format!("description: {}\n", entry.description));
    out.push_str(&format!("status: {}\n", entry.status));
    out.push_str(&format!("created: {}\n", entry.created));
    out.push_str(&format!("updated: {}\n", entry.updated));
    out.push_str(&format!("source: {}\n", entry.source));
    for (key, value) in &entry.extras {
        out.push_str(&format!("{key}: {value}\n"));
    }
    out.push_str("---\n\n");
    out.push_str(&entry.body);
    if !entry.body.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// 今天的日期(YYYY-MM-DD,UTC)。civil-from-days 算法,不引 chrono。
pub fn today() -> String {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let days = ms / 86_400_000;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Howard Hinnant 的 civil_from_days:epoch 天数 → (年,月,日)。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter_roundtrip_preserves_unknown_keys() {
        let entry = MemoryEntry {
            id: "M-013".into(),
            scope: "project".into(),
            category: "fact".into(),
            title: "edit 未命中的头号原因是 CRLF 差异".into(),
            description: "处理 edit 替换失败、换行符相关问题时必读".into(),
            status: "active".into(),
            created: "2026-08-07".into(),
            updated: "2026-08-08".into(),
            source: "run:ses_xxx/42".into(),
            extras: vec![("future_field".into(), "来自新版本".into())],
            body: "正文第一行\n\n第二段。".into(),
        };
        let text = render_entry(&entry);
        assert_eq!(parse_entry(&text), entry);
    }

    #[test]
    fn tolerant_parse_of_hand_edited_files() {
        // 没有 frontmatter 的裸文件:全文进 body,不炸。
        let bare = parse_entry("就是一段手写笔记\n");
        assert_eq!(bare.body, "就是一段手写笔记");
        assert_eq!(bare.status, "active");
        // 缺键、值里带冒号都宽容。
        let partial = parse_entry("---\nid: M-001\ntitle: 比例是 16:9 的问题\n---\nbody");
        assert_eq!(partial.id, "M-001");
        assert_eq!(partial.title, "比例是 16:9 的问题");
        assert_eq!(partial.body, "body");
    }

    #[test]
    fn slug_keeps_cjk_and_caps_length() {
        assert_eq!(slugify("发版 SOP: 两条通道"), "发版-sop-两条通道");
        assert_eq!(slugify("!!!"), "");
        assert!(slugify(&"字".repeat(100)).chars().count() <= 40);
    }

    #[test]
    fn civil_date_is_correct() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_672), (2026, 8, 7));
    }
}
