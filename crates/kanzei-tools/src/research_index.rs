//! R-277 批5：topic 级 Tantivy 全文索引、统一检索入口与断点续跑。

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Field, Schema, Value as TantivyValue, STORED, STRING, TEXT};
use tantivy::{doc, Index, IndexReader, TantivyDocument, Term};

use kanzei_harness::{Tool, ToolCtx, ToolOutput};

use crate::docstore::DocStore;
use crate::symbols::SymbolsTool;

const INDEX_DIR: &str = "index";
const CHECKPOINT_FILE: &str = "index_checkpoint.json";
const MAX_DOCUMENT_BYTES: u64 = 3 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct IndexCheckpoint {
    version: u32,
    topic: String,
    status: String,
    processed: usize,
    total: usize,
    next_path: Option<String>,
}

#[derive(Debug, Clone)]
struct SourceDocument {
    path: String,
    domain: String,
    body: String,
}

#[derive(Clone, Copy)]
struct IndexFields {
    path: Field,
    domain: Field,
    body: Field,
}

fn topic_dir(root: &Path, topic: &str) -> Result<PathBuf, String> {
    DocStore::validate_topic(topic).map_err(|error| error.to_string())?;
    Ok(root.join(".kanzei/research").join(topic))
}

fn checkpoint_path(dir: &Path) -> PathBuf {
    dir.join(CHECKPOINT_FILE)
}

fn atomic_write(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("创建索引目录失败: {error}"))?;
    }
    let _lock = kanzei_base::atomic_file::lock_exclusive(path)
        .map_err(|error| format!("锁定索引 checkpoint 失败: {error}"))?;
    kanzei_base::atomic_file::write_atomic(path, content)
        .map_err(|error| format!("写入索引 checkpoint 失败: {error}"))
}

fn save_checkpoint(dir: &Path, checkpoint: &IndexCheckpoint) -> Result<(), String> {
    let text = serde_json::to_string_pretty(checkpoint)
        .map_err(|error| format!("序列化索引 checkpoint 失败: {error}"))?;
    atomic_write(&checkpoint_path(dir), &text)
}

fn load_checkpoint(dir: &Path) -> Result<Option<IndexCheckpoint>, String> {
    let path = checkpoint_path(dir);
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("读取索引 checkpoint 失败: {error}"))?;
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|error| format!("索引 checkpoint JSON 无效: {error}"))
}

fn collect_rs_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if name == ".git" || name == ".kanzei" || name == "target" || name == "vendor" {
                    continue;
                }
                stack.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

fn collect_documents(root: &Path, dir: &Path) -> Vec<SourceDocument> {
    let mut documents = Vec::new();
    let source_dir = dir.join("source_text");
    if let Ok(entries) = std::fs::read_dir(source_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|extension| extension != "txt") {
                continue;
            }
            let Ok(metadata) = std::fs::metadata(&path) else {
                continue;
            };
            if metadata.len() > MAX_DOCUMENT_BYTES {
                continue;
            }
            let Ok(body) = std::fs::read_to_string(&path) else {
                continue;
            };
            let name = path.file_stem().unwrap_or_default().to_string_lossy();
            documents.push(SourceDocument {
                path: format!("source:{name}"),
                domain: "literature".into(),
                body,
            });
        }
    }
    for path in collect_rs_files(root) {
        let Ok(metadata) = std::fs::metadata(&path) else {
            continue;
        };
        if metadata.len() > MAX_DOCUMENT_BYTES {
            continue;
        }
        let Ok(body) = std::fs::read_to_string(&path) else {
            continue;
        };
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        documents.push(SourceDocument {
            path: relative,
            domain: "code".into(),
            body,
        });
    }
    documents.sort_by(|left, right| left.path.cmp(&right.path));
    documents
}

fn schema() -> (Schema, IndexFields) {
    let mut builder = Schema::builder();
    let path = builder.add_text_field("path", STRING | STORED);
    let domain = builder.add_text_field("domain", STRING | STORED);
    let body = builder.add_text_field("body", TEXT | STORED);
    let schema = builder.build();
    (schema, IndexFields { path, domain, body })
}

fn open_or_create_index(dir: &Path) -> Result<(Index, IndexFields), String> {
    let index_dir = dir.join(INDEX_DIR);
    let (schema, _fields) = schema();
    let index = if index_dir.join("meta.json").is_file() {
        Index::open_in_dir(&index_dir).map_err(|error| format!("打开 Tantivy 索引失败: {error}"))?
    } else {
        std::fs::create_dir_all(&index_dir)
            .map_err(|error| format!("创建 Tantivy 目录失败: {error}"))?;
        Index::create_in_dir(&index_dir, schema)
            .map_err(|error| format!("创建 Tantivy 索引失败: {error}"))?
    };
    let schema = index.schema();
    let fields = IndexFields {
        path: schema
            .get_field("path")
            .map_err(|error| format!("索引缺少 path 字段: {error}"))?,
        domain: schema
            .get_field("domain")
            .map_err(|error| format!("索引缺少 domain 字段: {error}"))?,
        body: schema
            .get_field("body")
            .map_err(|error| format!("索引缺少 body 字段: {error}"))?,
    };
    Ok((index, fields))
}

fn checkpoint_summary(checkpoint: &IndexCheckpoint) -> Value {
    json!({
        "topic": checkpoint.topic,
        "status": checkpoint.status,
        "processed": checkpoint.processed,
        "total": checkpoint.total,
        "next_path": checkpoint.next_path,
        "checkpoint": CHECKPOINT_FILE,
    })
}

fn search_index(dir: &Path, query: &str, limit: usize) -> Result<Value, String> {
    let (index, fields) = open_or_create_index(dir)?;
    let reader: IndexReader = index
        .reader()
        .map_err(|error| format!("打开 Tantivy reader 失败: {error}"))?;
    let searcher = reader.searcher();
    let parser = QueryParser::for_index(&index, vec![fields.body, fields.path]);
    let parsed = parser
        .parse_query(query)
        .map_err(|error| format!("全文查询语法错误: {error}"))?;
    let docs = searcher
        .search(&parsed, &TopDocs::with_limit(limit.clamp(1, 50)))
        .map_err(|error| format!("Tantivy 查询失败: {error}"))?;
    let mut hits = Vec::new();
    for (score, address) in docs {
        let document: TantivyDocument = searcher
            .doc(address)
            .map_err(|error| format!("读取索引文档失败: {error}"))?;
        let path = document
            .get_first(fields.path)
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let domain = document
            .get_first(fields.domain)
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let body = document
            .get_first(fields.body)
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let excerpt = body.chars().take(500).collect::<String>();
        hits.push(json!({ "path": path, "domain": domain, "score": score, "excerpt": excerpt }));
    }
    Ok(json!({ "query": query, "hits": hits, "count": hits.len() }))
}

pub struct ResearchIndexTool;

#[async_trait]
impl Tool for ResearchIndexTool {
    fn name(&self) -> &'static str {
        "research_index"
    }

    fn description(&self) -> String {
        "topic 级 Tantivy 索引与统一检索：index_build/index_resume 逐文件索引 source_text 与 Rust 代码并写 checkpoint；search 同一入口返回文献和代码命中；symbols mode 复用现有 symbols 反查并返回统一结果。".into()
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["index_build", "index_resume", "search", "symbols"] },
                "topic": { "type": "string", "pattern": "^[a-z0-9]+(?:-[a-z0-9]+)*$" },
                "query": { "type": "string" },
                "limit": { "type": "integer", "minimum": 1, "maximum": 50 },
                "path": { "type": "string" },
                "filter": { "type": "string" },
                "define": { "type": "string" }
            },
            "required": ["action", "topic"],
            "additionalProperties": false
        })
    }

    fn resources(&self, input: &Value) -> Vec<String> {
        let action = input
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        if matches!(action, "search" | "symbols") {
            vec![format!("read:{action}")]
        } else {
            vec![format!("write:{action}")]
        }
    }

    async fn execute(&self, input: Value, ctx: &ToolCtx) -> ToolOutput {
        let action = input.get("action").and_then(Value::as_str).unwrap_or("");
        let Some(topic) = input.get("topic").and_then(Value::as_str) else {
            return ToolOutput::needs_correction("MISSING_TOPIC", "research_index 必须提供 topic");
        };
        let dir = match topic_dir(&ctx.project_root, topic) {
            Ok(dir) => dir,
            Err(error) => return ToolOutput::needs_correction("INVALID_TOPIC", error),
        };
        match action {
            "index_build" | "index_resume" => {
                let documents = collect_documents(&ctx.project_root, &dir);
                let mut checkpoint = match load_checkpoint(&dir) {
                    Ok(Some(checkpoint)) if checkpoint.topic == topic => checkpoint,
                    Ok(Some(_)) | Ok(None) => IndexCheckpoint {
                        version: 1,
                        topic: topic.into(),
                        status: "running".into(),
                        processed: 0,
                        total: documents.len(),
                        next_path: None,
                    },
                    Err(error) => return ToolOutput::error(error),
                };
                if checkpoint.status == "complete" && action == "index_resume" {
                    return ToolOutput::ok(checkpoint_summary(&checkpoint).to_string());
                }
                checkpoint.status = "running".into();
                checkpoint.total = documents.len();
                if let Err(error) = save_checkpoint(&dir, &checkpoint) {
                    return ToolOutput::error(error);
                }
                let (index, fields) = match open_or_create_index(&dir) {
                    Ok(value) => value,
                    Err(error) => return ToolOutput::error(error),
                };
                let mut writer = match index.writer(50_000_000) {
                    Ok(writer) => writer,
                    Err(error) => {
                        return ToolOutput::error(format!("打开 Tantivy writer 失败: {error}"))
                    }
                };
                let start_path = checkpoint.next_path.clone();
                for document in documents {
                    if let Some(start_path) = &start_path {
                        if document.path <= *start_path {
                            continue;
                        }
                    }
                    writer.delete_term(Term::from_field_text(fields.path, &document.path));
                    if let Err(error) = writer.add_document(doc!(
                        fields.path => document.path.clone(),
                        fields.domain => document.domain,
                        fields.body => document.body
                    )) {
                        return ToolOutput::error(format!("写入 Tantivy 文档失败: {error}"));
                    }
                    if let Err(error) = writer.commit() {
                        return ToolOutput::error(format!("提交 Tantivy 文档失败: {error}"));
                    }
                    checkpoint.processed += 1;
                    checkpoint.next_path = Some(document.path);
                    if let Err(error) = save_checkpoint(&dir, &checkpoint) {
                        return ToolOutput::error(error);
                    }
                }
                if let Err(error) = writer.wait_merging_threads() {
                    return ToolOutput::error(format!("等待 Tantivy merge 失败: {error}"));
                }
                checkpoint.status = "complete".into();
                checkpoint.next_path = None;
                if let Err(error) = save_checkpoint(&dir, &checkpoint) {
                    return ToolOutput::error(error);
                }
                ToolOutput::ok(checkpoint_summary(&checkpoint).to_string())
            }
            "search" => {
                let query = input
                    .get("query")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim();
                if query.is_empty() {
                    return ToolOutput::needs_correction("EMPTY_QUERY", "search 必须提供 query");
                }
                let limit = input.get("limit").and_then(Value::as_u64).unwrap_or(10) as usize;
                match search_index(&dir, query, limit) {
                    Ok(result) => ToolOutput::ok(result.to_string()),
                    Err(error) => ToolOutput::error(error),
                }
            }
            "symbols" => {
                let mut symbols_input = json!({
                    "path": input.get("path").cloned().unwrap_or(Value::String(".".into())),
                    "filter": input.get("filter").cloned().unwrap_or(Value::Null),
                    "define": input.get("define").cloned().unwrap_or(Value::Null),
                });
                if symbols_input["filter"].is_null() {
                    symbols_input.as_object_mut().unwrap().remove("filter");
                }
                if symbols_input["define"].is_null() {
                    symbols_input.as_object_mut().unwrap().remove("define");
                }
                let symbols = SymbolsTool.execute(symbols_input, ctx).await;
                if symbols.is_error {
                    return symbols;
                }
                ToolOutput::ok(json!({ "topic": topic, "mode": "symbols", "indexed": dir.join(INDEX_DIR).display().to_string(), "result": symbols.content }).to_string())
            }
            _ => ToolOutput::needs_correction(
                "INVALID_ACTION",
                "action 只能是 index_build/index_resume/search/symbols",
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn root(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "kz-research-index-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(path.join(".kanzei/research/topic-a/source_text")).unwrap();
        std::fs::create_dir_all(path.join("crates/kanzei-tools/src")).unwrap();
        path
    }

    #[tokio::test]
    async fn one_index_searches_literature_and_code_and_resumes() {
        let project = root("unified");
        std::fs::write(
            project.join(".kanzei/research/topic-a/source_text/S-001.txt"),
            "tantivy literature bridge",
        )
        .unwrap();
        std::fs::write(
            project.join("crates/kanzei-tools/src/sample.rs"),
            "pub struct TantivyBridge;\nfn bridge_query() {}",
        )
        .unwrap();
        let ctx = ToolCtx::new(project.clone(), project.clone());
        let tool = ResearchIndexTool;
        let built = tool
            .execute(json!({ "action": "index_build", "topic": "topic-a" }), &ctx)
            .await;
        assert!(!built.is_error, "{}", built.content);
        let literature = tool
            .execute(
                json!({ "action": "search", "topic": "topic-a", "query": "literature" }),
                &ctx,
            )
            .await;
        assert!(
            literature.content.contains("source:S-001"),
            "{}",
            literature.content
        );
        let code = tool
            .execute(
                json!({ "action": "search", "topic": "topic-a", "query": "bridge_query" }),
                &ctx,
            )
            .await;
        assert!(
            code.content.contains("crates/kanzei-tools/src/sample.rs"),
            "{}",
            code.content
        );
        let symbols = tool.execute(json!({ "action": "symbols", "topic": "topic-a", "path": "crates/kanzei-tools/src/sample.rs", "filter": "TantivyBridge" }), &ctx).await;
        assert!(
            symbols.content.contains("TantivyBridge"),
            "{}",
            symbols.content
        );
        let symbols_error = tool
            .execute(
                json!({ "action": "symbols", "topic": "topic-a", "path": "missing.rs" }),
                &ctx,
            )
            .await;
        assert!(
            symbols_error.is_error,
            "底层 symbols 错误必须从统一入口传播"
        );
        let resumed = tool
            .execute(
                json!({ "action": "index_resume", "topic": "topic-a" }),
                &ctx,
            )
            .await;
        assert!(!resumed.is_error, "{}", resumed.content);
        let checkpoint: IndexCheckpoint = serde_json::from_str(
            &std::fs::read_to_string(
                project.join(".kanzei/research/topic-a/index_checkpoint.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(checkpoint.status, "complete");
        std::fs::remove_dir_all(project).ok();
    }

    #[tokio::test]
    async fn corrupt_checkpoint_is_reported_without_overwrite() {
        let project = root("corrupt");
        let checkpoint = project.join(".kanzei/research/topic-a/index_checkpoint.json");
        std::fs::write(&checkpoint, "not-json").unwrap();
        let ctx = ToolCtx::new(project.clone(), project.clone());
        let output = ResearchIndexTool
            .execute(
                json!({ "action": "index_resume", "topic": "topic-a" }),
                &ctx,
            )
            .await;
        assert!(output.is_error);
        assert_eq!(std::fs::read_to_string(checkpoint).unwrap(), "not-json");
        std::fs::remove_dir_all(project).ok();
    }
}
