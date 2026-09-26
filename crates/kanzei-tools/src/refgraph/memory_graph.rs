//! 记忆知识图谱投影(docs/design/memory_knowledge_graph.md §2-§3)。
//!
//! - [`collect_inputs`]:全部 IO——两级记忆库(活动 + archive/)、tracker 三类文档(活动 + 归档)、
//!   设计文档文件名、[`AreaRegistry`]。
//! - [`build_graph`]:纯函数,输入 → 节点/边/区域/统计/警告。
//! - [`input_fingerprint`]:输入文件 (相对路径, 长度, mtime) 的哈希,桌面端据此做 stat 缓存。
//!
//! 所有节点同一形状(可空字段显式为 null):IPC 契约只取数组首元素定形,形状不一会漏字段。

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use kanzei_harness::areas::{AreaKind, AreaRegistry};
use kanzei_harness::refs::{parse_id, split_refs, RefRel};
use serde::Serialize;

use super::mentions::{ids_in, line_of, wikilinks};
use super::tool_areas::{backticked_tools, keyword_areas, title_lead_tool, tool_area};
use crate::memory::{fp_markers, MemoryEntry, MemoryStore};

/// 每条记忆最多挂几个区域(再多就是噪声,画面上也读不出来)。
pub const MAX_AREAS_PER_MEMORY: usize = 4;
/// 经结构化 refs 指向的条目,正文路径命中的区域超过这个数就不采用(条目太宽,不说明记忆关于哪)。
pub const VIA_MAX_AREAS: usize = 3;
const WARNINGS_CAP: usize = 200;
const SCRIPTS_AREA: &str = "scripts";

#[derive(Debug, Clone)]
pub struct MemoryInput {
    pub entry: MemoryEntry,
    /// "project" | "global"
    pub scope: String,
    pub archived: bool,
    pub hits: u64,
    /// 锚点用的显示路径(项目内相对路径;全局库写 `~/.kanzei/memory/…`)。
    pub path: String,
    /// 文件原文(锚点行号按原文算;缺省时按 title/description/body 拼出来的文本算)。
    pub raw: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TrackerInput {
    pub id: String,
    /// "requirement" | "defect" | "decision"
    pub kind: &'static str,
    pub title: String,
    pub status: String,
    pub archived: bool,
    /// 条目全部字段值拼起来的文本(经 D-xxx 推断区域时扫路径)。
    pub text: String,
}

#[derive(Debug, Clone, Default)]
pub struct GraphInputs {
    pub memories: Vec<MemoryInput>,
    pub tracker: Vec<TrackerInput>,
    /// docs/design/ 下的文件名(`memory_control_plane.md`)。
    pub design_docs: Vec<String>,
    pub registry: AreaRegistry,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GraphNode {
    pub id: String,
    /// memory | requirement | defect | decision | doc | crate | module | fingerprint | subject
    pub kind: &'static str,
    pub label: String,
    pub title: String,
    pub description: Option<String>,
    pub scope: Option<String>,
    pub category: Option<String>,
    pub status: Option<String>,
    pub archived: bool,
    pub updated: Option<String>,
    pub hits: u64,
    pub areas: Vec<String>,
    pub primary_area: Option<String>,
    /// field | path | tool | via | keyword;无区域为 null。
    pub area_provenance: Option<String>,
    pub degree: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    /// refs | basis | implements | supersedes | derived_from | mentions | cites | about |
    /// has_fingerprint | has_subject | contains | depends_on
    pub rel: &'static str,
    /// strong | weak
    pub strength: &'static str,
    /// about 边的依据:field | path | tool | via | keyword
    pub provenance: Option<&'static str>,
    /// via 依据经过的条目(D-xxx)
    pub via: Option<String>,
    /// `<显示路径>:<行号>`
    pub anchor: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GraphArea {
    /// `area:<id>`
    pub id: String,
    pub kind: &'static str,
    pub label: String,
    pub parent: Option<String>,
    pub depth: u32,
    pub band: u32,
    /// 以它(或它的子模块)为区域的记忆数。
    pub memories: u32,
}

/// 主区域依据分布。固定键(而不是 map):IPC 契约按键定形,键集合不能随数据变。
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct ProvenanceCounts {
    pub field: u32,
    pub path: u32,
    pub tool: u32,
    pub via: u32,
    pub keyword: u32,
    pub none: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct GraphStats {
    pub memories: u32,
    pub live: u32,
    pub archived: u32,
    pub unassigned: u32,
    pub by_provenance: ProvenanceCounts,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MemoryGraph {
    pub version: u32,
    pub generated_at: u64,
    pub build_ms: u64,
    /// hit | miss(桌面端缓存层填写)
    pub cache: String,
    pub areas: Vec<GraphArea>,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub stats: GraphStats,
    pub warnings: Vec<String>,
}

fn area_node_id(id: &str) -> String {
    format!("area:{id}")
}

fn is_live_status(status: &str) -> bool {
    matches!(status, "active" | "candidate" | "shadow")
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Provenance {
    Field,
    Path,
    Tool,
    Via,
    Keyword,
}

impl Provenance {
    fn as_str(self) -> &'static str {
        match self {
            Provenance::Field => "field",
            Provenance::Path => "path",
            Provenance::Tool => "tool",
            Provenance::Via => "via",
            Provenance::Keyword => "keyword",
        }
    }
    fn strong(self) -> bool {
        matches!(self, Provenance::Field | Provenance::Path)
    }
}

/// 文本里的代码路径 / Rust 路径 / 前端脚本名 → 区域(去重,按出现顺序)。
pub fn path_areas(registry: &AreaRegistry, text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let is_path_char =
        |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | '\\' | ':' | '-');
    for raw in text.split(|c: char| !is_path_char(c)) {
        let token = raw.trim_matches(|c: char| matches!(c, '.' | ':' | '-'));
        if token.len() < 4 || token.contains("://") {
            continue;
        }
        let script =
            token.ends_with(".js") && token.as_bytes().first().is_some_and(u8::is_ascii_digit);
        if !(token.contains('/') || token.contains('\\') || token.contains("::") || script) {
            continue;
        }
        if let Some(area) = registry.resolve_token(token) {
            if !out.contains(&area) {
                out.push(area);
            }
        }
    }
    out
}

struct Builder<'a> {
    inputs: &'a GraphInputs,
    nodes: BTreeMap<String, GraphNode>,
    edges: Vec<GraphEdge>,
    edge_keys: BTreeSet<(String, String, &'static str)>,
    warnings: Vec<String>,
}

fn blank_node(id: String, kind: &'static str, label: String, title: String) -> GraphNode {
    GraphNode {
        id,
        kind,
        label,
        title,
        description: None,
        scope: None,
        category: None,
        status: None,
        archived: false,
        updated: None,
        hits: 0,
        areas: Vec::new(),
        primary_area: None,
        area_provenance: None,
        degree: 0,
    }
}

impl Builder<'_> {
    fn edge(
        &mut self,
        source: &str,
        target: &str,
        rel: &'static str,
        strength: &'static str,
    ) -> Option<&mut GraphEdge> {
        if source == target
            || !self
                .edge_keys
                .insert((source.to_string(), target.to_string(), rel))
        {
            return None;
        }
        self.edges.push(GraphEdge {
            source: source.to_string(),
            target: target.to_string(),
            rel,
            strength,
            provenance: None,
            via: None,
            anchor: None,
        });
        self.edges.last_mut()
    }

    fn warn(&mut self, text: String) {
        if self.warnings.len() < WARNINGS_CAP {
            self.warnings.push(text);
        }
    }

    fn tracker_node(&mut self, id: &str) -> bool {
        if self.nodes.contains_key(id) {
            return true;
        }
        let Some(entry) = self.inputs.tracker.iter().find(|t| t.id == id) else {
            return false;
        };
        let mut node = blank_node(
            id.to_string(),
            entry.kind,
            id.to_string(),
            entry.title.clone(),
        );
        node.status = Some(entry.status.clone());
        node.archived = entry.archived;
        self.nodes.insert(id.to_string(), node);
        true
    }

    fn doc_node(&mut self, name: &str) -> Option<String> {
        if !self.inputs.design_docs.iter().any(|d| d == name) {
            return None;
        }
        let id = format!("doc:{name}");
        self.nodes.entry(id.clone()).or_insert_with(|| {
            blank_node(
                id.clone(),
                "doc",
                name.trim_end_matches(".md").to_string(),
                format!("docs/design/{name}"),
            )
        });
        Some(id)
    }
}

/// 纯函数:输入 → 图。规则见 docs/design/memory_knowledge_graph.md §3。
pub fn build_graph(inputs: &GraphInputs) -> MemoryGraph {
    let registry = &inputs.registry;
    let mut builder = Builder {
        inputs,
        nodes: BTreeMap::new(),
        edges: Vec::new(),
        edge_keys: BTreeSet::new(),
        warnings: Vec::new(),
    };

    // 同一 id 活动优先(与 doc_reference_graph §1 的 M-009 规则一致)。
    let mut memories: BTreeMap<String, &MemoryInput> = BTreeMap::new();
    for memory in &inputs.memories {
        match memories.get(&memory.entry.id) {
            Some(existing) if !existing.archived => {}
            _ => {
                memories.insert(memory.entry.id.clone(), memory);
            }
        }
    }
    let title_to_id: HashMap<&str, &str> = memories
        .values()
        .map(|m| (m.entry.title.as_str(), m.entry.id.as_str()))
        .collect();
    // 条目正文路径 → 区域(via 用)。
    let tracker_paths: HashMap<&str, Vec<String>> = inputs
        .tracker
        .iter()
        .map(|t| (t.id.as_str(), path_areas(registry, &t.text)))
        .collect();

    let mut about: BTreeMap<String, Vec<(String, Provenance, Option<String>)>> = BTreeMap::new();
    // 概念键 → (标签, 种类, 全文)
    let mut concept_members: BTreeMap<String, (String, &'static str, String)> = BTreeMap::new();
    let mut concept_use: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut stats = GraphStats::default();

    for memory in memories.values() {
        let entry = &memory.entry;
        let id = entry.id.clone();
        let text = memory
            .raw
            .clone()
            .unwrap_or_else(|| format!("{}\n{}\n{}", entry.title, entry.description, entry.body));
        let anchor =
            |needle: &str| line_of(&text, needle).map(|line| format!("{}:{line}", memory.path));
        let mut node = blank_node(id.clone(), "memory", id.clone(), entry.title.clone());
        node.description = Some(entry.description.clone());
        node.scope = Some(memory.scope.clone());
        node.category = Some(entry.category.clone());
        node.status = Some(entry.status.clone());
        node.archived = memory.archived;
        node.updated = Some(entry.updated.clone());
        node.hits = memory.hits;
        builder.nodes.insert(id.clone(), node);
        stats.memories += 1;
        if memory.archived {
            stats.archived += 1;
        } else if is_live_status(&entry.status) {
            stats.live += 1;
        }

        // ── 区域候选 ──
        let mut found: Vec<(String, Provenance, Option<String>)> = Vec::new();
        for token in entry.areas() {
            match registry.resolve_token(&token) {
                Some(area) => found.push((area, Provenance::Field, None)),
                None => builder.warn(format!(
                    "{id}: area 字段 `{token}` 在当前项目里解析不到区域"
                )),
            }
        }
        for area in path_areas(
            registry,
            &format!("{}\n{}\n{}", entry.title, entry.description, entry.body),
        ) {
            found.push((area, Provenance::Path, None));
        }
        let mut fingerprints: Vec<String> =
            fp_markers(&format!("{}\n{}", entry.description, entry.body));
        if let Some(field) = entry.field("fingerprint") {
            fingerprints.push(field.to_string());
        }
        let mut tools: Vec<&str> = fingerprints
            .iter()
            .filter_map(|fp| fp.trim_start_matches("[fp:").split('|').next())
            .collect();
        if let Some(lead) = title_lead_tool(&entry.title) {
            tools.push(lead);
        }
        tools.extend(backticked_tools(&entry.body));
        tools.extend(backticked_tools(&entry.description));
        for tool in tools {
            if let Some(area) = tool_area(tool).and_then(|a| registry.get(a).map(|a| a.id.clone()))
            {
                found.push((area, Provenance::Tool, None));
            }
        }

        // ── 结构化 refs:强边 + via ──
        let mut structured: BTreeSet<String> = BTreeSet::new();
        let refs_field = entry.field("refs").unwrap_or_default().to_string();
        for token in split_refs(&refs_field).tokens {
            let rel = token.rel.as_str();
            let value = token.token.as_str();
            if let Some((prefix, _)) = parse_id(value) {
                match prefix {
                    "R" | "D" | "A" => {
                        if builder.tracker_node(value) {
                            if let Some(edge) = builder.edge(&id, value, rel, "strong") {
                                edge.anchor = anchor(value);
                            }
                            structured.insert(value.to_string());
                            if let Some(areas) = tracker_paths.get(value) {
                                if !areas.is_empty() && areas.len() <= VIA_MAX_AREAS {
                                    for area in areas {
                                        found.push((
                                            area.clone(),
                                            Provenance::Via,
                                            Some(value.to_string()),
                                        ));
                                    }
                                }
                            }
                        } else {
                            builder.warn(format!("{id}: refs 悬空 {value}(活动与归档里都没有)"));
                        }
                    }
                    "M" | "U" => {
                        structured.insert(value.to_string());
                        if memories.contains_key(value) {
                            if let Some(edge) = builder.edge(&id, value, rel, "strong") {
                                edge.anchor = anchor(value);
                            }
                        } else {
                            builder.warn(format!("{id}: refs 悬空 {value}"));
                        }
                    }
                    _ => {}
                }
                continue;
            }
            let normalized = value.replace('\\', "/");
            if let Some(name) = normalized.strip_prefix("docs/design/") {
                match builder.doc_node(name) {
                    Some(doc) => {
                        if let Some(edge) = builder.edge(&id, &doc, rel, "strong") {
                            edge.anchor = anchor(value);
                        }
                    }
                    None => builder.warn(format!("{id}: refs 指向不存在的设计文档 {value}")),
                }
                continue;
            }
            if let Some(area) = registry.resolve_token(value) {
                found.push((area, Provenance::Path, None));
            }
        }
        // supersedes / superseded_by 合成同一条 新→旧 边。
        let older = entry.supersedes().unwrap_or_default().to_string();
        for token in split_refs(&older).tokens {
            if matches!(parse_id(&token.token), Some(("M" | "U", _))) {
                structured.insert(token.token.clone());
                if memories.contains_key(token.token.as_str()) {
                    builder.edge(&id, &token.token, RefRel::Supersedes.as_str(), "strong");
                }
            }
        }
        if let Some(newer) = entry.field("superseded_by") {
            for token in split_refs(newer).tokens {
                if matches!(parse_id(&token.token), Some(("M" | "U", _))) {
                    structured.insert(token.token.clone());
                    if memories.contains_key(token.token.as_str()) {
                        builder.edge(&token.token, &id, RefRel::Supersedes.as_str(), "strong");
                    }
                }
            }
        }

        // ── 弱边:正文提及、设计文档文件名、[[标题]] ──
        let prose = format!("{}\n{}\n{}", entry.title, entry.description, entry.body);
        for mention in ids_in(&prose, "RDAMU") {
            if mention.id == id || structured.contains(&mention.id) {
                continue;
            }
            let is_memory = mention.id.starts_with("M-") || mention.id.starts_with("U-");
            let exists = if is_memory {
                memories.contains_key(mention.id.as_str())
            } else {
                builder.tracker_node(&mention.id)
            };
            if exists {
                if let Some(edge) = builder.edge(&id, &mention.id, "mentions", "weak") {
                    edge.anchor = anchor(&mention.id);
                }
            }
        }
        for doc in &inputs.design_docs {
            if prose.contains(doc.as_str()) {
                if let Some(doc_id) = builder.doc_node(doc) {
                    let doc_anchor = anchor(doc);
                    if let Some(edge) = builder.edge(&id, &doc_id, "cites", "weak") {
                        edge.anchor = doc_anchor;
                    }
                }
            }
        }
        for (title, _line) in wikilinks(&prose) {
            match title_to_id.get(title.as_str()) {
                Some(target) => {
                    let target = target.to_string();
                    builder.edge(&id, &target, "mentions", "weak");
                }
                None => builder.warn(format!("{id}: 无法解析的 [[{title}]]")),
            }
        }

        // ── 概念:失败指纹与 subject ──
        for fp in fingerprints.iter().collect::<BTreeSet<_>>() {
            let inner = fp.trim_start_matches("[fp:").trim_end_matches(']').trim();
            if inner.is_empty() {
                continue;
            }
            let key = format!("fp:{}", inner.chars().take(80).collect::<String>());
            let (tool, head) = inner.split_once('|').unwrap_or((inner, ""));
            let label = format!("{tool}｜{}", head.chars().take(18).collect::<String>());
            concept_members
                .entry(key.clone())
                .or_insert((label, "fingerprint", inner.to_string()));
            concept_use.entry(key).or_default().push(id.clone());
        }
        if let Some(subject) = entry.field("subject") {
            let key = format!("subject:{subject}");
            let label: String = subject.chars().take(20).collect();
            concept_members
                .entry(key.clone())
                .or_insert((label, "subject", subject.to_string()));
            concept_use.entry(key).or_default().push(id.clone());
        }

        // ── 关键词兜底(总是加入候选,weak)──
        for area in keyword_areas(&prose) {
            if let Some(area) = registry.get(area) {
                found.push((area.id.clone(), Provenance::Keyword, None));
            }
        }
        about.insert(id, found);
    }

    // ── 区域选择:每个区域取最高档依据,scripts 降权,最多 MAX_AREAS_PER_MEMORY 个 ──
    let mut used_areas: BTreeSet<String> = BTreeSet::new();
    let mut area_memories: BTreeMap<String, u32> = BTreeMap::new();
    for (id, found) in &about {
        let mut best: BTreeMap<String, (Provenance, Option<String>)> = BTreeMap::new();
        for (area, provenance, via) in found {
            match best.get(area) {
                Some((current, _)) if current <= provenance => {}
                _ => {
                    best.insert(area.clone(), (*provenance, via.clone()));
                }
            }
        }
        let demote = |area: &str| usize::from(registry.crate_of(area) == Some(SCRIPTS_AREA)) * 10;
        let mut ranked: Vec<(String, Provenance, Option<String>)> = best
            .into_iter()
            .map(|(area, (provenance, via))| (area, provenance, via))
            .collect();
        ranked.sort_by(|a, b| {
            (a.1 as usize + demote(&a.0))
                .cmp(&(b.1 as usize + demote(&b.0)))
                .then_with(|| a.0.cmp(&b.0))
        });
        ranked.truncate(MAX_AREAS_PER_MEMORY);
        let counts = &mut stats.by_provenance;
        match ranked.first().map(|r| r.1) {
            Some(Provenance::Field) => counts.field += 1,
            Some(Provenance::Path) => counts.path += 1,
            Some(Provenance::Tool) => counts.tool += 1,
            Some(Provenance::Via) => counts.via += 1,
            Some(Provenance::Keyword) => counts.keyword += 1,
            None => counts.none += 1,
        }
        if ranked.is_empty() {
            stats.unassigned += 1;
        }
        let mut counted: BTreeSet<String> = BTreeSet::new();
        if let Some(node) = builder.nodes.get_mut(id) {
            node.areas = ranked.iter().map(|r| area_node_id(&r.0)).collect();
            node.primary_area = ranked.first().map(|r| area_node_id(&r.0));
            node.area_provenance = ranked.first().map(|r| r.1.as_str().to_string());
        }
        for (area, provenance, via) in ranked {
            let target = area_node_id(&area);
            let strength = if provenance.strong() {
                "strong"
            } else {
                "weak"
            };
            if let Some(edge) = builder.edge(id, &target, "about", strength) {
                edge.provenance = Some(provenance.as_str());
                edge.via = via;
            }
            counted.insert(area.clone());
            if let Some(crate_id) = registry.crate_of(&area) {
                counted.insert(crate_id.to_string());
            }
            used_areas.insert(area);
        }
        for area in counted {
            *area_memories.entry(area).or_default() += 1;
        }
    }

    // ── 区域节点:全部 crate 级 + 被记忆 about 的模块;contains / depends_on ──
    for area in registry.all() {
        let emit = area.kind == AreaKind::Crate || used_areas.contains(&area.id);
        if !emit {
            continue;
        }
        let node_id = area_node_id(&area.id);
        let mut node = blank_node(
            node_id.clone(),
            area.kind.as_str(),
            area.label.clone(),
            area.id.clone(),
        );
        node.areas = vec![node_id.clone()];
        node.primary_area = Some(area_node_id(
            registry.crate_of(&area.id).unwrap_or(&area.id),
        ));
        builder.nodes.insert(node_id.clone(), node);
    }
    for area in registry.all() {
        let node_id = area_node_id(&area.id);
        if !builder.nodes.contains_key(&node_id) {
            continue;
        }
        if let Some(parent) = &area.parent {
            let parent_id = area_node_id(parent);
            if builder.nodes.contains_key(&parent_id) {
                builder.edge(&parent_id, &node_id, "contains", "strong");
            }
        }
    }
    for (from, to) in registry.crate_deps() {
        builder.edge(
            &area_node_id(&from),
            &area_node_id(&to),
            "depends_on",
            "strong",
        );
    }

    // ── 概念:只保留被 ≥2 条记忆共享的 ──
    for (key, members) in &concept_use {
        let unique: BTreeSet<&String> = members.iter().collect();
        if unique.len() < 2 {
            continue;
        }
        let Some((label, kind, title)) = concept_members.get(key).cloned() else {
            continue;
        };
        builder
            .nodes
            .insert(key.clone(), blank_node(key.clone(), kind, label, title));
        let rel = if kind == "fingerprint" {
            "has_fingerprint"
        } else {
            "has_subject"
        };
        for member in unique {
            builder.edge(member, key, rel, "strong");
        }
    }

    // ── 收尾:丢掉悬空边,算度数 ──
    let nodes = &builder.nodes;
    let edges: Vec<GraphEdge> = builder
        .edges
        .into_iter()
        .filter(|e| nodes.contains_key(&e.source) && nodes.contains_key(&e.target))
        .collect();
    let mut degree: HashMap<&str, u32> = HashMap::new();
    for edge in &edges {
        *degree.entry(edge.source.as_str()).or_default() += 1;
        *degree.entry(edge.target.as_str()).or_default() += 1;
    }
    let mut nodes: Vec<GraphNode> = builder.nodes.into_values().collect();
    for node in &mut nodes {
        node.degree = degree.get(node.id.as_str()).copied().unwrap_or(0);
    }
    let areas = registry
        .all()
        .iter()
        .map(|area| GraphArea {
            id: area_node_id(&area.id),
            kind: area.kind.as_str(),
            label: area.label.clone(),
            parent: area.parent.as_deref().map(area_node_id),
            depth: area.depth,
            band: area.band,
            memories: area_memories.get(&area.id).copied().unwrap_or(0),
        })
        .collect();
    MemoryGraph {
        version: 1,
        generated_at: 0,
        build_ms: 0,
        cache: "miss".into(),
        areas,
        nodes,
        edges,
        stats,
        warnings: builder.warnings,
    }
}

fn display_path(root: &Path, path: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(relative) => relative.display().to_string().replace('\\', "/"),
        Err(_) => {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let archived = path
                .parent()
                .and_then(|p| p.file_name())
                .is_some_and(|n| n == "archive");
            format!(
                "~/.kanzei/memory/{}{name}",
                if archived { "archive/" } else { "" }
            )
        }
    }
}

/// 项目库 + 全局库(桌面端与 CLI 的默认口径)。
pub fn memory_stores(root: &Path) -> Vec<MemoryStore> {
    let mut stores = vec![MemoryStore::project(root)];
    stores.extend(MemoryStore::global());
    stores
}

/// 全部 IO:读两级记忆库(含 archive/)、tracker 三类文档(含归档)、设计文档名、区域注册表。
pub fn collect_inputs(root: &Path) -> GraphInputs {
    collect_inputs_from(root, &memory_stores(root))
}

/// 同 [`collect_inputs`],记忆库由调用方给(契约测试只给项目库,不读本机全局库)。
pub fn collect_inputs_from(root: &Path, stores: &[MemoryStore]) -> GraphInputs {
    use crate::docstore::{DocStore, DECISIONS, DEFECTS, REQUIREMENTS};
    let registry = AreaRegistry::scan(root);
    let mut memories = Vec::new();
    for store in stores {
        let scope = store.scope.label().to_string();
        let hits = store.hit_profile();
        for (archived, list) in [(false, store.load_all()), (true, store.load_archived())] {
            for (path, entry) in list {
                memories.push(MemoryInput {
                    hits: hits.get(&entry.id).map(|h| h.0).unwrap_or(0),
                    raw: std::fs::read_to_string(&path).ok(),
                    path: display_path(root, &path),
                    scope: scope.clone(),
                    archived,
                    entry,
                });
            }
        }
    }
    let mut tracker = Vec::new();
    for (kind, doc) in [
        ("requirement", &REQUIREMENTS),
        ("defect", &DEFECTS),
        ("decision", &DECISIONS),
    ] {
        let store = DocStore::open(root, doc);
        for (archived, entries) in [
            (false, store.load().unwrap_or_default()),
            (true, store.load_archive().unwrap_or_default()),
        ] {
            for entry in entries {
                if tracker.iter().any(|t: &TrackerInput| t.id == entry.id) {
                    continue;
                }
                let text = entry
                    .fields
                    .iter()
                    .map(|(k, v)| format!("{k}: {v}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                tracker.push(TrackerInput {
                    id: entry.id,
                    kind,
                    title: entry.title,
                    status: entry.status,
                    archived,
                    text,
                });
            }
        }
    }
    let mut design_docs: Vec<String> = std::fs::read_dir(root.join("docs/design"))
        .map(|dir| {
            dir.flatten()
                .filter_map(|item| {
                    let name = item.file_name().to_string_lossy().to_string();
                    name.ends_with(".md").then_some(name)
                })
                .collect()
        })
        .unwrap_or_default();
    design_docs.sort();
    GraphInputs {
        memories,
        tracker,
        design_docs,
        registry,
    }
}

fn stat_into(hasher: &mut std::collections::hash_map::DefaultHasher, path: &Path) {
    path.hash(hasher);
    if let Ok(meta) = std::fs::metadata(path) {
        meta.len().hash(hasher);
        if let Ok(modified) = meta.modified() {
            modified.hash(hasher);
        }
    } else {
        0u8.hash(hasher);
    }
}

/// 目录里的 Markdown 文件(名 + 长度 + mtime)。只看 .md:index.db / 锁文件 / WAL 是派生物,
/// 读图谱本身(命中数查询)就可能创建或改动它们,算进来缓存就永远不命中。
fn md_dir_into(hasher: &mut std::collections::hash_map::DefaultHasher, dir: &Path) {
    dir.hash(hasher);
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = read
        .flatten()
        .map(|item| item.path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("md"))
        .collect();
    paths.sort();
    for path in paths {
        stat_into(hasher, &path);
    }
}

/// 注册表相关目录:只看条目名(区域只取决于有哪些文件/子目录),Cargo.toml 另算内容戳(依赖边)。
/// 不看其余文件的 mtime:改代码不该让图谱缓存失效。
fn names_into(hasher: &mut std::collections::hash_map::DefaultHasher, dir: &Path) {
    dir.hash(hasher);
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut names: Vec<(String, bool)> = read
        .flatten()
        .map(|item| {
            (
                item.file_name().to_string_lossy().to_string(),
                item.file_type().map(|t| t.is_dir()).unwrap_or(false),
            )
        })
        .collect();
    names.sort();
    for (name, is_dir) in names {
        name.hash(hasher);
        is_dir.hash(hasher);
        if name.eq_ignore_ascii_case("cargo.toml") {
            stat_into(hasher, &dir.join(&name));
        }
    }
}

/// 输入指纹:任何一个输入文件增删改(长度或 mtime 变),值就变。桌面端 stat 缓存的键。
/// 不含 index.db:命中数由调用方每次现取覆盖,不让它拖垮缓存命中率。
pub fn input_fingerprint(root: &Path) -> u64 {
    use crate::docstore::{DocStore, DECISIONS, DEFECTS, REQUIREMENTS};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for store in memory_stores(root) {
        md_dir_into(&mut hasher, &store.root);
        md_dir_into(&mut hasher, &store.root.join("archive"));
    }
    for doc in [&REQUIREMENTS, &DEFECTS, &DECISIONS] {
        let store = DocStore::open(root, doc);
        stat_into(&mut hasher, &root.join(doc.rel_path));
        stat_into(&mut hasher, &store.archive_file());
    }
    md_dir_into(&mut hasher, &root.join("docs/design"));
    for dir in AreaRegistry::scan(root).watch_dirs(root) {
        names_into(&mut hasher, &dir);
    }
    hasher.finish()
}
