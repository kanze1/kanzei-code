//! Research 实验事实模型与 Markdown 真源(R-343)。
//!
//! 这里仅负责从 `.kanzei/research/<topic>/explorations/*.md` 重建探索图和结果行。
//! Markdown 与产物目录是唯一真源；不存在物化索引，也不从参数文本推断任何语义。

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ResearchError {
    #[error("research topic 非法: {0}")]
    InvalidTopic(String),
    #[error("research 标识符非法: {0}")]
    InvalidIdentifier(String),
    #[error("读取 research 路径 {path} 失败: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExplorationStatus {
    Draft,
    Running,
    Done,
    Abandoned,
}

impl ExplorationStatus {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "draft" => Some(Self::Draft),
            "running" => Some(Self::Running),
            "done" => Some(Self::Done),
            "abandoned" => Some(Self::Abandoned),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExperimentResultStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl ExperimentResultStatus {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "queued" => Some(Self::Queued),
            "running" => Some(Self::Running),
            "succeeded" => Some(Self::Succeeded),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExplorationFrontmatter {
    pub kind: String,
    pub id: String,
    pub topic: String,
    pub title: String,
    pub status: ExplorationStatus,
    pub hypothesis: String,
    pub depends_on: Vec<String>,
    pub supersedes: Option<String>,
    pub entry_refs: Vec<String>,
    pub environment: Option<String>,
    pub budget: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExperimentResult {
    pub result_id: String,
    pub params_text: String,
    pub status: ExperimentResultStatus,
    pub key_metrics_text: String,
    pub artifact_text: String,
    pub conclusion: String,
    pub artifact_dir: String,
    pub source_line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExplorationDocument {
    pub source_path: String,
    pub frontmatter: ExplorationFrontmatter,
    pub assumption: String,
    pub results: Vec<ExperimentResult>,
    pub conclusion: String,
    pub follow_up: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResearchDiagnostic {
    pub path: String,
    pub line: usize,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResearchTopic {
    pub topic: String,
    pub explorations: Vec<ExplorationDocument>,
    pub diagnostics: Vec<ResearchDiagnostic>,
}
impl ResearchTopic {
    /// 返回 topic 内下一个探索编号；仅规划编号，不执行并发写入或自动创建文件。
    pub fn next_exploration_id(&self) -> String {
        let next = self
            .explorations
            .iter()
            .filter_map(|document| {
                document
                    .frontmatter
                    .id
                    .strip_prefix("E-")
                    .and_then(|value| value.parse::<u64>().ok())
            })
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        format!("E-{next:03}")
    }
}

impl ExplorationDocument {
    /// 返回本探索内下一个结果编号；参数仍是自由文本，且不会触碰任何产物。
    pub fn next_result_id(&self) -> String {
        let prefix = format!("{}-", self.frontmatter.id);
        let next = self
            .results
            .iter()
            .filter_map(|result| result.result_id.strip_prefix(&prefix))
            .filter_map(|value| value.parse::<u64>().ok())
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        format!("{}-{next:02}", self.frontmatter.id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParsedExploration {
    pub document: ExplorationDocument,
    pub diagnostics: Vec<ResearchDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResultArtifactSkeleton {
    pub result_dir: PathBuf,
    pub environment_path: PathBuf,
    pub stdout_path: PathBuf,
    pub metrics_path: PathBuf,
    pub artifacts_dir: PathBuf,
}

/// 从一个探索 Markdown 重建结构化事实；解析错误保留在 diagnostics，不静默丢失文件。
pub fn parse_exploration_markdown(path: &Path, text: &str) -> ParsedExploration {
    let (frontmatter, body_start_line, body, mut diagnostics) = match parse_frontmatter(path, text)
    {
        Ok(value) => value,
        Err(diagnostics) => {
            return ParsedExploration {
                document: empty_document(path),
                diagnostics,
            };
        }
    };
    let mut fields = frontmatter;

    let kind = required_field(&mut fields, "kind", path, &mut diagnostics, body_start_line);
    let id = required_field(&mut fields, "id", path, &mut diagnostics, body_start_line);
    let topic = required_field(
        &mut fields,
        "topic",
        path,
        &mut diagnostics,
        body_start_line,
    );
    let title = required_field(
        &mut fields,
        "title",
        path,
        &mut diagnostics,
        body_start_line,
    );
    let status_text = required_field(
        &mut fields,
        "status",
        path,
        &mut diagnostics,
        body_start_line,
    );
    let hypothesis = required_field(
        &mut fields,
        "hypothesis",
        path,
        &mut diagnostics,
        body_start_line,
    );
    let created_at = required_integer(
        &mut fields,
        "created_at",
        path,
        &mut diagnostics,
        body_start_line,
    );
    let updated_at = required_integer(
        &mut fields,
        "updated_at",
        path,
        &mut diagnostics,
        body_start_line,
    );

    if kind != "exploration" {
        diagnostic(
            &mut diagnostics,
            path,
            frontmatter_line(&fields, "kind", body_start_line),
            format!("kind 必须为 exploration，实际为 `{kind}`"),
        );
    }
    if !is_exploration_id(&id) {
        diagnostic(
            &mut diagnostics,
            path,
            frontmatter_line(&fields, "id", body_start_line),
            format!("id 必须匹配 E-<n>，实际为 `{id}`"),
        );
    }
    let status = match ExplorationStatus::parse(&status_text) {
        Some(status) => status,
        None => {
            diagnostic(
                &mut diagnostics,
                path,
                frontmatter_line(&fields, "status", body_start_line),
                format!("status 非法 `{status_text}`，允许 draft/running/done/abandoned"),
            );
            ExplorationStatus::Draft
        }
    };

    let depends_on = split_list(fields.remove("depends_on").unwrap_or_default());
    for dependency in &depends_on {
        if !is_exploration_id(dependency) {
            diagnostic(
                &mut diagnostics,
                path,
                frontmatter_line(&fields, "depends_on", body_start_line),
                format!("depends_on 标识符非法 `{dependency}`，必须匹配 E-<n>"),
            );
        }
    }
    let supersedes = optional_field(&mut fields, "supersedes");
    if let Some(value) = &supersedes {
        if !is_exploration_id(value) {
            diagnostic(
                &mut diagnostics,
                path,
                frontmatter_line(&fields, "supersedes", body_start_line),
                format!("supersedes 标识符非法 `{value}`，必须匹配 E-<n>"),
            );
        }
    }
    let entry_refs = split_list(fields.remove("entry_refs").unwrap_or_default());
    for entry_ref in &entry_refs {
        if !is_tracker_ref(entry_ref) {
            diagnostic(
                &mut diagnostics,
                path,
                frontmatter_line(&fields, "entry_refs", body_start_line),
                format!("entry_refs 必须是 R-/D-/T- 编号，实际为 `{entry_ref}`"),
            );
        }
    }
    let environment = optional_field(&mut fields, "environment");
    let budget = optional_field(&mut fields, "budget");

    let lines: Vec<&str> = body.lines().collect();
    let mut section_titles = BTreeSet::new();
    for (index, line) in lines.iter().enumerate() {
        if let Some(title) = line.trim().strip_prefix("## ") {
            if !section_titles.insert(title.trim().to_string()) {
                diagnostic(
                    &mut diagnostics,
                    path,
                    body_start_line + index,
                    format!("固定段落标题重复 `## {}`", title.trim()),
                );
            }
        }
    }
    let sections = find_sections(&lines, body_start_line);
    let assumption = required_section(&sections, "假设", path, &mut diagnostics, body_start_line);
    let conclusion = required_section(&sections, "结论", path, &mut diagnostics, body_start_line);
    let follow_up = required_section(&sections, "后续", path, &mut diagnostics, body_start_line);
    let results = parse_results(&sections, &id, path, &mut diagnostics, body_start_line);

    ParsedExploration {
        document: ExplorationDocument {
            source_path: path_to_string(path),
            frontmatter: ExplorationFrontmatter {
                kind,
                id,
                topic,
                title,
                status,
                hypothesis,
                depends_on,
                supersedes,
                entry_refs,
                environment,
                budget,
                created_at,
                updated_at,
            },
            assumption,
            results,
            conclusion,
            follow_up,
        },
        diagnostics,
    }
}

/// 从一个 topic 的 Markdown 真源完整重建探索及结果；不会读取或写入派生索引。
pub fn load_research_topic(
    project_root: &Path,
    topic: &str,
) -> Result<ResearchTopic, ResearchError> {
    validate_topic(topic)?;
    let topic_dir = project_root.join(".kanzei").join("research").join(topic);
    let explorations_dir = topic_dir.join("explorations");
    let mut paths = Vec::new();
    match fs::read_dir(&explorations_dir) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry.map_err(|source| ResearchError::Io {
                    path: explorations_dir.clone(),
                    source,
                })?;
                let path = entry.path();
                if path.extension().is_some_and(|extension| extension == "md") {
                    paths.push(path);
                }
            }
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(ResearchError::Io {
                path: explorations_dir,
                source,
            });
        }
    }
    paths.sort();

    let mut explorations = Vec::new();
    let mut diagnostics = Vec::new();
    for path in paths {
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(source) => {
                diagnostic(
                    &mut diagnostics,
                    &path,
                    1,
                    format!("读取 Markdown 失败: {source}"),
                );
                continue;
            }
        };
        let parsed = parse_exploration_markdown(&path, &text);
        diagnostics.extend(parsed.diagnostics);
        explorations.push(parsed.document);
    }

    validate_topic_documents(topic, &explorations, &mut diagnostics);
    Ok(ResearchTopic {
        topic: topic.to_string(),
        explorations,
        diagnostics,
    })
}

/// 创建单条结果的产物目录骨架；不创建派生索引，也不覆盖已有产物。
pub fn ensure_result_artifact_skeleton(
    topic_dir: &Path,
    exploration_id: &str,
    result_id: &str,
) -> Result<ResultArtifactSkeleton, ResearchError> {
    if !is_exploration_id(exploration_id) {
        return Err(ResearchError::InvalidIdentifier(exploration_id.to_string()));
    }
    if !is_result_id_for(result_id, exploration_id) {
        return Err(ResearchError::InvalidIdentifier(result_id.to_string()));
    }
    let result_dir = topic_dir
        .join("explorations")
        .join(exploration_id)
        .join(result_id);
    let artifacts_dir = result_dir.join("artifacts");
    fs::create_dir_all(&artifacts_dir).map_err(|source| ResearchError::Io {
        path: artifacts_dir.clone(),
        source,
    })?;
    Ok(ResultArtifactSkeleton {
        environment_path: result_dir.join("environment.json"),
        stdout_path: result_dir.join("stdout.log"),
        metrics_path: result_dir.join("metrics.jsonl"),
        result_dir,
        artifacts_dir,
    })
}

fn validate_topic(topic: &str) -> Result<(), ResearchError> {
    let valid = !topic.is_empty()
        && !topic.starts_with('-')
        && !topic.ends_with('-')
        && !topic.contains("--")
        && topic.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        });
    if valid {
        Ok(())
    } else {
        Err(ResearchError::InvalidTopic(topic.to_string()))
    }
}

type FrontmatterParse = Result<
    (
        BTreeMap<String, String>,
        usize,
        String,
        Vec<ResearchDiagnostic>,
    ),
    Vec<ResearchDiagnostic>,
>;

fn parse_frontmatter(path: &Path, text: &str) -> FrontmatterParse {
    let lines: Vec<&str> = text.lines().collect();
    let mut diagnostics = Vec::new();
    if lines.first().map(|line| line.trim()) != Some("---") {
        diagnostic(
            &mut diagnostics,
            path,
            1,
            "缺少以 --- 开始的 frontmatter".to_string(),
        );
        return Err(diagnostics);
    }
    let Some(close_index) = lines
        .iter()
        .enumerate()
        .skip(1)
        .find_map(|(index, line)| (line.trim() == "---").then_some(index))
    else {
        diagnostic(
            &mut diagnostics,
            path,
            lines.len().max(1),
            "frontmatter 缺少结束分隔符 ---".to_string(),
        );
        return Err(diagnostics);
    };

    let mut fields = BTreeMap::new();
    for (index, line) in lines[1..close_index].iter().enumerate() {
        let line_number = index + 2;
        let Some((key, value)) = line.split_once(':') else {
            diagnostic(
                &mut diagnostics,
                path,
                line_number,
                "frontmatter 行必须是 key: value".to_string(),
            );
            continue;
        };
        let key = key.trim().to_string();
        if key.is_empty() {
            diagnostic(
                &mut diagnostics,
                path,
                line_number,
                "frontmatter key 不能为空".to_string(),
            );
            continue;
        }
        if fields
            .insert(key.clone(), value.trim().to_string())
            .is_some()
        {
            diagnostic(
                &mut diagnostics,
                path,
                line_number,
                format!("frontmatter 字段重复 `{key}`"),
            );
        }
    }
    let body_start_line = close_index + 2;
    let body = lines.get(close_index + 1..).unwrap_or_default().join("\n");
    Ok((fields, body_start_line, body, diagnostics))
}

fn required_field(
    fields: &mut BTreeMap<String, String>,
    key: &str,
    path: &Path,
    diagnostics: &mut Vec<ResearchDiagnostic>,
    line: usize,
) -> String {
    match fields.remove(key) {
        Some(value) if !value.trim().is_empty() => value,
        Some(_) => {
            diagnostic(
                diagnostics,
                path,
                frontmatter_line(fields, key, line),
                format!("缺少必填字段 `{key}` 的值"),
            );
            String::new()
        }
        None => {
            diagnostic(
                diagnostics,
                path,
                line.saturating_sub(1).max(1),
                format!("缺少必填字段 `{key}`"),
            );
            String::new()
        }
    }
}

fn required_integer(
    fields: &mut BTreeMap<String, String>,
    key: &str,
    path: &Path,
    diagnostics: &mut Vec<ResearchDiagnostic>,
    line: usize,
) -> i64 {
    let value = required_field(fields, key, path, diagnostics, line);
    match value.parse::<i64>() {
        Ok(value) => value,
        Err(_) if value.is_empty() => 0,
        Err(_) => {
            diagnostic(
                diagnostics,
                path,
                frontmatter_line(fields, key, line),
                format!("字段 `{key}` 必须是 unix_ms 整数，实际为 `{value}`"),
            );
            0
        }
    }
}

fn optional_field(fields: &mut BTreeMap<String, String>, key: &str) -> Option<String> {
    fields.remove(key).filter(|value| !value.trim().is_empty())
}

fn split_list(value: String) -> Vec<String> {
    value.split_whitespace().map(ToString::to_string).collect()
}

fn find_sections<'a>(lines: &[&'a str], body_start_line: usize) -> BTreeMap<String, Section<'a>> {
    let mut sections = BTreeMap::new();
    let mut current: Option<(String, usize)> = None;
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if let Some(title) = trimmed.strip_prefix("## ") {
            if let Some((old_title, start)) = current.take() {
                sections.insert(
                    old_title,
                    Section {
                        start_line: body_start_line + start,
                        lines: lines[start..index].to_vec(),
                    },
                );
            }
            current = Some((title.trim().to_string(), index + 1));
        }
    }
    if let Some((title, start)) = current {
        sections.insert(
            title,
            Section {
                start_line: body_start_line + start,
                lines: lines[start..].to_vec(),
            },
        );
    }
    sections
}

#[derive(Debug)]
struct Section<'a> {
    start_line: usize,
    lines: Vec<&'a str>,
}

fn required_section(
    sections: &BTreeMap<String, Section<'_>>,
    title: &str,
    path: &Path,
    diagnostics: &mut Vec<ResearchDiagnostic>,
    line: usize,
) -> String {
    match sections.get(title) {
        Some(section) => section.lines.join("\n").trim().to_string(),
        None => {
            diagnostic(
                diagnostics,
                path,
                line.max(1),
                format!("缺少固定段落 `## {title}`"),
            );
            String::new()
        }
    }
}

fn parse_results(
    sections: &BTreeMap<String, Section<'_>>,
    exploration_id: &str,
    path: &Path,
    diagnostics: &mut Vec<ResearchDiagnostic>,
    line: usize,
) -> Vec<ExperimentResult> {
    let Some(section) = sections.get("实验结果") else {
        diagnostic(
            diagnostics,
            path,
            line.max(1),
            "缺少固定段落 `## 实验结果`，无法解析结果表".to_string(),
        );
        return Vec::new();
    };
    let table_lines: Vec<(usize, &str)> = section
        .lines
        .iter()
        .enumerate()
        .filter(|(_, value)| value.trim_start().starts_with('|'))
        .map(|(index, value)| (section.start_line + index, *value))
        .collect();
    let Some((header_line, header)) = table_lines
        .iter()
        .find(|(_, value)| value.trim_start().starts_with('|'))
    else {
        diagnostic(
            diagnostics,
            path,
            section.start_line,
            "实验结果段必须包含六列表格".to_string(),
        );
        return Vec::new();
    };
    let headers = table_cells(header);
    let expected = ["实验", "参数", "状态", "关键指标", "产物", "结论"];
    if headers.len() != expected.len()
        || headers
            .iter()
            .map(String::as_str)
            .ne(expected.iter().copied())
    {
        diagnostic(
            diagnostics,
            path,
            *header_line,
            format!("实验结果表头必须是 `{}` 六列", expected.join(" | ")),
        );
        return Vec::new();
    }

    let header_index = table_lines
        .iter()
        .position(|(line_number, _)| line_number == header_line)
        .unwrap_or(0);
    let Some((separator_line, separator)) = table_lines.get(header_index + 1) else {
        diagnostic(
            diagnostics,
            path,
            *header_line,
            "实验结果表缺少分隔行".to_string(),
        );
        return Vec::new();
    };
    let separator_cells = table_cells(separator);
    if separator_cells.len() != 6
        || separator_cells.iter().any(|cell| {
            cell.trim_matches(':')
                .chars()
                .filter(|character| *character == '-')
                .count()
                < 3
        })
    {
        diagnostic(
            diagnostics,
            path,
            *separator_line,
            "实验结果表分隔行必须有六个 --- 单元格".to_string(),
        );
        return Vec::new();
    }

    let mut results = Vec::new();
    for (line_number, row) in table_lines.iter().skip(header_index + 2) {
        let cells = table_cells(row);
        if cells.len() != 6 {
            diagnostic(
                diagnostics,
                path,
                *line_number,
                format!("实验结果行必须有六列，实际为 {} 列", cells.len()),
            );
            continue;
        }
        let result_id = cells[0].clone();
        if !is_result_id_for(&result_id, exploration_id) {
            diagnostic(
                diagnostics,
                path,
                *line_number,
                format!("结果 id `{result_id}` 必须匹配 `{exploration_id}-<nn>`"),
            );
        }
        let status = match ExperimentResultStatus::parse(&cells[2]) {
            Some(status) => status,
            None => {
                diagnostic(
                    diagnostics,
                    path,
                    *line_number,
                    format!(
                        "结果状态非法 `{}`，允许 queued/running/succeeded/failed/cancelled",
                        cells[2]
                    ),
                );
                ExperimentResultStatus::Failed
            }
        };
        results.push(ExperimentResult {
            artifact_dir: format!("explorations/{exploration_id}/{result_id}"),
            result_id,
            params_text: cells[1].clone(),
            status,
            key_metrics_text: cells[3].clone(),
            artifact_text: cells[4].clone(),
            conclusion: cells[5].clone(),
            source_line: *line_number,
        });
    }
    results
}

fn table_cells(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let inner = trimmed
        .strip_prefix('|')
        .and_then(|value| value.strip_suffix('|'))
        .unwrap_or(trimmed);
    inner
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect()
}

fn validate_topic_documents(
    topic: &str,
    explorations: &[ExplorationDocument],
    diagnostics: &mut Vec<ResearchDiagnostic>,
) {
    let mut exploration_paths = HashMap::new();
    let mut all_ids: HashMap<String, String> = HashMap::new();
    let valid_ids: BTreeSet<String> = explorations
        .iter()
        .filter(|document| is_exploration_id(&document.frontmatter.id))
        .map(|document| document.frontmatter.id.clone())
        .collect();

    for document in explorations {
        let frontmatter = &document.frontmatter;
        if frontmatter.topic != topic {
            diagnostics.push(ResearchDiagnostic {
                path: document.source_path.clone(),
                line: 1,
                message: format!("topic 必须为 `{topic}`，实际为 `{}`", frontmatter.topic),
            });
        }
        if let Some(previous) =
            exploration_paths.insert(frontmatter.id.clone(), document.source_path.clone())
        {
            diagnostics.push(ResearchDiagnostic {
                path: document.source_path.clone(),
                line: 1,
                message: format!("探索 id `{}` 与 `{previous}` 重复", frontmatter.id),
            });
        }
        if is_exploration_id(&frontmatter.id) {
            all_ids.insert(frontmatter.id.clone(), document.source_path.clone());
        }
        for result in &document.results {
            let result_key = result.result_id.clone();
            if let Some(previous) = all_ids.insert(result_key.clone(), document.source_path.clone())
            {
                diagnostics.push(ResearchDiagnostic {
                    path: document.source_path.clone(),
                    line: result.source_line,
                    message: format!("结果 id `{result_key}` 与 `{previous}` 重复"),
                });
            }
        }
    }

    let mut graph = HashMap::new();
    let mut paths = HashMap::new();
    for document in explorations {
        let id = &document.frontmatter.id;
        if graph.contains_key(id) {
            continue;
        }
        paths.insert(id.clone(), document.source_path.clone());
        for dependency in &document.frontmatter.depends_on {
            if !valid_ids.contains(dependency) {
                diagnostics.push(ResearchDiagnostic {
                    path: document.source_path.clone(),
                    line: 1,
                    message: format!("depends_on 引用悬挂探索 `{dependency}`"),
                });
            }
        }
        graph.insert(id.clone(), document.frontmatter.depends_on.clone());
    }
    let mut states = HashMap::new();
    let mut stack = Vec::new();
    for id in valid_ids {
        detect_cycle(&id, &graph, &paths, &mut states, &mut stack, diagnostics);
    }
}

fn detect_cycle(
    id: &str,
    graph: &HashMap<String, Vec<String>>,
    paths: &HashMap<String, String>,
    states: &mut HashMap<String, u8>,
    stack: &mut Vec<String>,
    diagnostics: &mut Vec<ResearchDiagnostic>,
) {
    match states.get(id).copied() {
        Some(1) => {
            let start = stack.iter().position(|value| value == id).unwrap_or(0);
            let mut cycle = stack[start..].to_vec();
            cycle.push(id.to_string());
            diagnostics.push(ResearchDiagnostic {
                path: paths.get(id).cloned().unwrap_or_default(),
                line: 1,
                message: format!("depends_on 存在成环: {}", cycle.join(" -> ")),
            });
            return;
        }
        Some(2) => return,
        _ => {}
    }
    states.insert(id.to_string(), 1);
    stack.push(id.to_string());
    if let Some(dependencies) = graph.get(id) {
        for dependency in dependencies {
            if graph.contains_key(dependency) {
                detect_cycle(dependency, graph, paths, states, stack, diagnostics);
            }
        }
    }
    stack.pop();
    states.insert(id.to_string(), 2);
}

fn is_exploration_id(value: &str) -> bool {
    value.strip_prefix("E-").is_some_and(|suffix| {
        !suffix.is_empty() && suffix.chars().all(|character| character.is_ascii_digit())
    })
}

fn is_result_id_for(value: &str, exploration_id: &str) -> bool {
    let Some(suffix) = value.strip_prefix(&format!("{exploration_id}-")) else {
        return false;
    };
    suffix.len() == 2 && suffix.chars().all(|character| character.is_ascii_digit())
}

fn is_tracker_ref(value: &str) -> bool {
    ["R-", "D-", "T-"].iter().any(|prefix| {
        value.strip_prefix(prefix).is_some_and(|suffix| {
            !suffix.is_empty() && suffix.chars().all(|character| character.is_ascii_digit())
        })
    })
}

fn empty_document(path: &Path) -> ExplorationDocument {
    ExplorationDocument {
        source_path: path_to_string(path),
        frontmatter: ExplorationFrontmatter {
            kind: String::new(),
            id: String::new(),
            topic: String::new(),
            title: String::new(),
            status: ExplorationStatus::Draft,
            hypothesis: String::new(),
            depends_on: Vec::new(),
            supersedes: None,
            entry_refs: Vec::new(),
            environment: None,
            budget: None,
            created_at: 0,
            updated_at: 0,
        },
        assumption: String::new(),
        results: Vec::new(),
        conclusion: String::new(),
        follow_up: String::new(),
    }
}

fn frontmatter_line(fields: &BTreeMap<String, String>, key: &str, fallback: usize) -> usize {
    // 字段 map 只保存值，不保存行号；所有诊断仍落在 frontmatter 区域，保证可定位且不伪造正文行。
    let _ = fields.get(key);
    fallback.saturating_sub(1).max(1)
}

fn diagnostic(
    diagnostics: &mut Vec<ResearchDiagnostic>,
    path: &Path,
    line: usize,
    message: String,
) {
    diagnostics.push(ResearchDiagnostic {
        path: path_to_string(path),
        line: line.max(1),
        message,
    });
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_root(label: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("kanzei-research-{label}-{suffix}"));
        fs::create_dir_all(root.join(".kanzei/research")).unwrap();
        root
    }

    fn exploration(id: &str, depends_on: &str) -> String {
        format!(
            "---\nkind: exploration\nid: {id}\ntopic: nas-search\ntitle: {id} title\nstatus: running\nhypothesis: {id} can beat baseline\ndepends_on: {depends_on}\nsupersedes:\nentry_refs: R-343\nenvironment: ENV-gpu01\nbudget: 20 gpu-hour\ncreated_at: 1788230400000\nupdated_at: 1788256800000\n---\n\n## 假设\n\n{id} hypothesis\n\n## 实验结果\n\n| 实验 | 参数 | 状态 | 关键指标 | 产物 | 结论 |\n| --- | --- | --- | --- | --- | --- |\n| {id}-01 | seed=7 lr=3e-4 ops=3 | succeeded | test_acc 0.938 | [产物]({id}/{id}-01/) | 超过基线 |\n\n## 结论\n\n等待更多结果。\n\n## 后续\n\n继续验证。\n"
        )
    }

    #[test]
    fn loads_explorations_results_and_preserves_parameter_text() {
        let root = test_root("valid");
        let dir = root.join(".kanzei/research/nas-search/explorations");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("E-001.md"), exploration("E-001", "")).unwrap();
        fs::write(dir.join("E-002.md"), exploration("E-002", "E-001")).unwrap();

        let topic = load_research_topic(&root, "nas-search").unwrap();
        assert_eq!(topic.explorations.len(), 2);
        assert!(topic.diagnostics.is_empty(), "{:?}", topic.diagnostics);
        assert_eq!(topic.explorations[0].results.len(), 1);
        assert_eq!(
            topic.explorations[0].results[0].params_text,
            "seed=7 lr=3e-4 ops=3"
        );
        assert_eq!(
            topic.explorations[0].results[0].artifact_dir,
            "explorations/E-001/E-001-01"
        );
        assert_eq!(topic.explorations[1].frontmatter.depends_on, vec!["E-001"]);
        assert_eq!(topic.next_exploration_id(), "E-003");
        assert_eq!(topic.explorations[0].next_result_id(), "E-001-02");
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn diagnoses_missing_fields_and_bad_enum_values_with_source_location() {
        let path = PathBuf::from(".kanzei/research/nas-search/explorations/E-001.md");
        let text = exploration("E-001", "")
            .replace("status: running", "status: paused")
            .replace("hypothesis: E-001 can beat baseline\n", "");
        let parsed = parse_exploration_markdown(&path, &text);
        assert!(parsed.diagnostics.iter().any(|diagnostic| {
            diagnostic.message.contains("status 非法")
                && diagnostic.path.ends_with("explorations/E-001.md")
                && diagnostic.line > 0
        }));
        assert!(parsed
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("缺少必填字段 `hypothesis`")));
    }

    #[test]
    fn reports_duplicate_dangling_and_cyclic_dependencies() {
        let root = test_root("diagnostics");
        let dir = root.join(".kanzei/research/nas-search/explorations");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("E-001.md"), exploration("E-001", "E-002 E-404")).unwrap();
        fs::write(dir.join("E-002.md"), exploration("E-002", "E-001")).unwrap();
        fs::write(dir.join("duplicate.md"), exploration("E-001", "")).unwrap();

        let topic = load_research_topic(&root, "nas-search").unwrap();
        let messages = topic
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>();
        assert!(
            messages.iter().any(|message| message.contains("重复")),
            "{messages:?}"
        );
        assert!(
            messages.iter().any(|message| message.contains("悬挂")),
            "{messages:?}"
        );
        assert!(
            messages.iter().any(|message| message.contains("成环")),
            "{messages:?}"
        );
        assert!(topic
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.line > 0));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn rebuilds_from_markdown_and_artifact_directories_without_index() {
        let root = test_root("rebuild");
        let topic_dir = root.join(".kanzei/research/nas-search");
        let explorations = topic_dir.join("explorations");
        fs::create_dir_all(&explorations).unwrap();
        fs::write(explorations.join("E-001.md"), exploration("E-001", "")).unwrap();
        let skeleton = ensure_result_artifact_skeleton(&topic_dir, "E-001", "E-001-01").unwrap();
        assert!(skeleton.result_dir.is_dir());
        assert!(skeleton.artifacts_dir.is_dir());
        fs::write(topic_dir.join("index.json"), "derived and disposable").unwrap();
        let before = load_research_topic(&root, "nas-search").unwrap();
        fs::remove_file(topic_dir.join("index.json")).unwrap();
        let after = load_research_topic(&root, "nas-search").unwrap();
        assert_eq!(before, after);
        assert_eq!(
            after.explorations[0].results[0].artifact_dir,
            "explorations/E-001/E-001-01"
        );
        fs::remove_dir_all(root).ok();
    }
}
