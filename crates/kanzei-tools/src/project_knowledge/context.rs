//! Budgeted module context: nearby contracts precede old incidents and summaries.
use std::cmp::Ordering;
use std::path::Path;

use super::{KnowledgeLink, KnowledgeMemory, ProjectKnowledge};
use crate::memory::{parse_entry, query_relevance};

struct Budget {
    text: String,
    used: usize,
    limit: usize,
    folded: usize,
    changed: usize,
}

impl Budget {
    fn push(&mut self, text: &str, limit: usize) -> bool {
        let cost = text.chars().count();
        if self.used + cost > limit.min(self.limit) {
            return false;
        }
        self.text.push_str(text);
        self.used += cost;
        true
    }
}

fn link_rank(link: &KnowledgeLink, target: &str) -> u8 {
    if link.strength != "strong" {
        4
    } else if link.area == target {
        0
    } else if link.area.starts_with(&format!("{target}/")) {
        1
    } else if target.starts_with(&format!("{}/", link.area)) {
        2
    } else {
        3
    }
}

fn kind_rank(memory: &KnowledgeMemory) -> u8 {
    match memory.kind.as_str() {
        "contract" => 0,
        "responsibility" => 1,
        "constraint" => 2,
        _ => 3,
    }
}

fn memory_context(
    knowledge: &ProjectKnowledge,
    target: &str,
    query: Option<&str>,
    out: &mut Budget,
) {
    let selected = knowledge.neighborhood(target);
    let mut records = Vec::new();
    for memory in &knowledge.memories {
        if memory.status != "active"
            || !memory.missing_areas.is_empty()
            || !((memory.links.is_empty() && ProjectKnowledge::boundary_memory(memory))
                || memory.links.iter().any(|link| {
                    knowledge.relevant_link(
                        link,
                        &selected,
                        target,
                        ProjectKnowledge::boundary_memory(memory),
                    )
                }))
        {
            continue;
        }
        let Ok(raw) =
            std::fs::read_to_string(Path::new(&knowledge.project_root).join(&memory.path))
        else {
            out.changed += 1;
            continue;
        };
        if crate::content_hash(raw.as_bytes()) != memory.revision {
            out.changed += 1;
            continue;
        }
        let entry = parse_entry(&raw);
        let relevance = query
            .and_then(|query| query_relevance(query, &entry))
            .unwrap_or(0.0);
        let rank = memory
            .links
            .iter()
            .filter(|link| {
                knowledge.relevant_link(
                    link,
                    &selected,
                    target,
                    ProjectKnowledge::boundary_memory(memory),
                )
            })
            .map(|link| link_rank(link, target))
            .min()
            .unwrap_or(2);
        records.push((memory, entry.body, relevance, rank));
    }
    records.sort_by(|(a, _, qa, ra), (b, _, qb, rb)| {
        // Target/descendant strong links form one tier, then ancestors, neighbors, weak guesses.
        ra.saturating_sub(1)
            .cmp(&rb.saturating_sub(1))
            .then_with(|| qb.partial_cmp(qa).unwrap_or(Ordering::Equal))
            .then_with(|| kind_rank(a).cmp(&kind_rank(b)))
            .then_with(|| ra.cmp(rb))
            .then_with(|| a.id.cmp(&b.id))
    });
    // Preserve space for purpose summaries; bodies never become partial constraints.
    let limit = out.limit * 4 / 5;
    for (memory, body, _, _) in records {
        let links = memory
            .links
            .iter()
            .filter(|link| {
                knowledge.relevant_link(
                    link,
                    &selected,
                    target,
                    ProjectKnowledge::boundary_memory(memory),
                )
            })
            .map(|link| format!("{}:{}:{}", link.area, link.provenance, link.strength))
            .collect::<Vec<_>>()
            .join(", ");
        let record = format!(
            "\n{} [{}] {}\n适用: {}\n关联: {links}\n来源: {}; file: {}\nrefs: {}\n{}\n",
            memory.id,
            memory.kind,
            memory.title,
            memory.description,
            memory.source,
            memory.path,
            memory.refs.join(" "),
            body.trim()
        );
        if !out.push(&record, limit) {
            out.folded += 1;
            out.push(
                &format!(
                    "{} {} · 正文未加载: {}\n",
                    memory.id, memory.title, memory.path
                ),
                limit,
            );
        }
    }
}

fn structure(knowledge: &ProjectKnowledge, target: &str, out: &mut Budget) {
    let selected = knowledge.neighborhood(target);
    let mut areas: Vec<_> = knowledge
        .areas
        .iter()
        .filter(|area| selected.contains(&area.id))
        .collect();
    areas.sort_by_key(|area| {
        (
            area.id != target,
            !target.starts_with(&format!("{}/", area.id)),
            area.tree_depth,
        )
    });
    for area in areas {
        let line = format!(
            "{} · parent={} · 依赖={} · 被依赖={}\n",
            area.id,
            area.parent.as_deref().unwrap_or("project"),
            area.dependencies.join(","),
            area.dependents.join(",")
        );
        if !out.push(&line, out.limit / 6) {
            out.folded += 1;
        }
    }
}

fn purposes(knowledge: &ProjectKnowledge, target: &str, token: &str, out: &mut Budget) {
    let selected = knowledge.neighborhood(target);
    let mut rows: Vec<_> = knowledge
        .areas
        .iter()
        .filter(|area| selected.contains(&area.id))
        .flat_map(|area| area.purposes.iter().map(move |purpose| (area, purpose)))
        .collect();
    let path = token.replace('\\', "/");
    rows.sort_by_key(|(area, purpose)| {
        (
            purpose.path != path,
            area.id != target,
            !area.id.starts_with(&format!("{target}/")),
            &purpose.path,
        )
    });
    for (_, purpose) in rows {
        let line = format!("用途说明 [{}; AI 摘要]: {}\n", purpose.path, purpose.text);
        if !out.push(&line, out.limit) {
            out.folded += 1;
        }
    }
}

pub(super) fn render(
    knowledge: &ProjectKnowledge,
    area: Option<&str>,
    query: Option<&str>,
    budget: usize,
) -> Result<String, String> {
    if !knowledge.enabled {
        return Err("此项目未启用层级项目知识；可在项目架构页开启并初始化。".into());
    }
    let Some(token) = area else {
        return Ok(knowledge.overview(budget));
    };
    let id = knowledge.resolve_area(token).ok_or_else(|| {
        format!("未找到项目区域 `{token}`；使用 architecture action=context 查看可用区域。")
    })?;
    let header = format!(
        "<project-context area=\"{id}\">\n代码树: {}\n",
        knowledge.code_root
    );
    // Footer has a fixed reserve; reject impossible budgets instead of truncating facts.
    if header.chars().count() + 160 > budget {
        return Err("上下文预算不足以容纳区域、来源和折叠说明。".into());
    }
    let mut out = Budget {
        used: 0,
        text: String::new(),
        limit: budget - 160,
        folded: 0,
        changed: 0,
    };
    out.push(&header, out.limit);
    structure(knowledge, &id, &mut out);
    memory_context(
        knowledge,
        &id,
        query.filter(|q| !q.trim().is_empty()),
        &mut out,
    );
    purposes(knowledge, &id, token, &mut out);
    out.text.push_str(&format!("\n预算未加载: {} 项；来源修订不匹配或不可读: {} 项。弱关联为推断，职责和边界须核对来源与代码。\n</project-context>", out.folded, out.changed));
    Ok(out.text)
}
