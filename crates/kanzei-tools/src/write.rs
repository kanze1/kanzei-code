//! write 工具。设计红线 5:结构化文本写入后做语法校验,坏格式以 warning 告知模型。

use async_trait::async_trait;
use kanzei_harness::{Tool, ToolConcurrency, ToolCtx, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize, JsonSchema)]
struct WriteInput {
    /// 文件路径(绝对或相对 cwd)
    #[serde(alias = "file_path", alias = "filepath", alias = "file")]
    path: String,
    /// 完整文件内容
    #[serde(alias = "text", alias = "contents")]
    content: String,
}

pub struct WriteTool;

/// R-268/D-395:专用写工具(write/edit/insert)写成功后记写日志。
///
/// 写日志是 bash 围栏收口对账的归因凭据:跨树围栏看到其它线树里的文件变了,
/// 查日志即可区分「该线专用工具的合法自写」与「他线 shell 越界写」。
/// `path` 用相对 `ctx.cwd` 的路径(跨树快照的 key 就是相对树根的路径,而
/// worktree 线的 cwd 就是树根,两端口径天然一致)。先写文档再记日志(「写后」
/// 凭据,见 write_log 模块头契约)。
pub(crate) fn record_worktree_write_log(ctx: &ToolCtx, rel_path: &str, content: &[u8]) {
    if ctx.run_id.is_none() || ctx.project_root.as_os_str().is_empty() {
        return;
    }
    let key = rel_path.replace('\\', "/");
    let _ = crate::write_log::record(
        &ctx.project_root,
        &crate::write_log::WriteLogEntry {
            at_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or_default(),
            path: key,
            fingerprint: crate::content_hash(content),
            content: content.to_vec(),
            run_id: ctx.run_id.clone(),
            process_id: ctx.process_id.clone(),
        },
    );
}

/// R-366 B1:带身份的写入先登记文件前像,写后记后像哈希。
///
/// 审计 18:前像、落盘、后像整段放进一个 spawn_blocking——稳态只 open 一次 state.db,
/// 连接不跨 await,SQLite/哈希/fsync 也不占 tokio worker。登记失败只 warn,不阻断写入。
/// 审计 19(1):写前 open 失败时,落盘后再 open 一次,只为记后像/占哨兵行;落盘后那次
/// open 或后像记录也失败时才不留行(边界见 kanzei-core store/file_checkpoints.rs 模块头)。
pub(crate) async fn file_checkpointed_write(
    ctx: &ToolCtx,
    path: &Path,
    bytes: &[u8],
) -> std::io::Result<()> {
    let run_id = match ctx.run_id.clone() {
        Some(run_id) if !ctx.project_root.as_os_str().is_empty() => run_id,
        _ => return tokio::fs::write(path, bytes).await,
    };
    let project_root = ctx.project_root.clone();
    let cwd = ctx.cwd.clone();
    let process_id = ctx.process_id.clone();
    let path = path.to_path_buf();
    let bytes = bytes.to_vec();
    tokio::task::spawn_blocking(move || {
        file_checkpointed_write_blocking(
            &project_root,
            &cwd,
            &run_id,
            process_id.as_deref(),
            &path,
            &bytes,
        )
    })
    .await
    .map_err(std::io::Error::other)?
}

fn file_checkpointed_write_blocking(
    project_root: &Path,
    cwd: &Path,
    run_id: &str,
    process_id: Option<&str>,
    path: &Path,
    bytes: &[u8],
) -> std::io::Result<()> {
    file_checkpointed_write_blocking_with(
        kanzei_core::store::SessionStore::open,
        project_root,
        cwd,
        run_id,
        process_id,
        path,
        bytes,
    )
}

/// `open_store` 可注入(测试用它模拟 state.db 打开失败),其余与生产路径完全一致。
fn file_checkpointed_write_blocking_with(
    open_store: impl Fn(
        &Path,
    ) -> Result<kanzei_core::store::SessionStore, kanzei_core::store::StoreError>,
    project_root: &Path,
    cwd: &Path,
    run_id: &str,
    process_id: Option<&str>,
    path: &Path,
    bytes: &[u8],
) -> std::io::Result<()> {
    // 审计 17:tree_root 记代码树根(不是 ctx.cwd),rel_path 由 store 按树根现算。
    let tree_root = kanzei_core::store::file_checkpoint_tree_root(cwd, project_root);
    let target = kanzei_core::store::FileCheckpointTarget {
        project_root,
        run_id,
        process_id,
        tree_root: &tree_root,
        abs_path: path,
    };
    let state_path = kanzei_core::store::project_state_path(project_root);
    let store = match open_store(&state_path) {
        Ok(store) => Some(store),
        Err(error) => {
            tracing::warn!(
                error = %error,
                run_id = %run_id,
                path = %path.display(),
                "file checkpoint store open failed; preimage skipped, will retry once after write"
            );
            None
        }
    };
    if let Some(store) = &store {
        if let Err(error) = kanzei_core::store::capture_file_preimage_in(store, &target) {
            tracing::warn!(
                error = %error,
                run_id = %run_id,
                path = %path.display(),
                "file checkpoint preimage capture failed; write continues"
            );
        }
    }

    std::fs::write(path, bytes)?;
    // 审计 19(1):写前没打开库时,落盘后再试一次——只为记后像;本 run/path 还没有行
    // 就占哨兵行,挡住同 run 后续触碰把这次写出的中间态补采成前像。稳态不走这里。
    let store = match store {
        Some(store) => store,
        None => match open_store(&state_path) {
            Ok(store) => store,
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    run_id = %run_id,
                    path = %path.display(),
                    "file checkpoint store reopen failed; no sentinel row, later touches in this run may capture intermediate state"
                );
                return Ok(());
            }
        },
    };
    if let Err(error) = kanzei_core::store::record_file_postimage_in(&store, &target, bytes) {
        tracing::warn!(
            error = %error,
            run_id = %run_id,
            path = %path.display(),
            "file checkpoint postimage record failed; write already succeeded"
        );
    }
    Ok(())
}

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &'static str {
        "write"
    }

    fn description(&self) -> String {
        "Write a file (overwrites). Params: path, content.".into()
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(WriteInput)).unwrap()
    }

    fn resources(&self, input: &serde_json::Value) -> Vec<String> {
        vec![input["path"].as_str().unwrap_or("*").to_string()]
    }

    fn concurrency(&self, _input: &serde_json::Value, ctx: &ToolCtx) -> ToolConcurrency {
        ToolConcurrency::write_worktree(ctx)
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolCtx) -> ToolOutput {
        let input: WriteInput = match crate::parse_input(self, input) {
            Ok(v) => v,
            Err(out) => return out,
        };
        let path = ctx
            .cwd
            .join(kanzei_harness::permission::normalize_resource(&input.path));
        if let Some(parent) = path.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                return ToolOutput::error(format!("cannot create {}: {e}", parent.display()));
            }
        }
        // 覆写前抓旧内容,给 UI 出 diff(看得见改了什么,R-015)。
        let previous = tokio::fs::read_to_string(&path).await.ok();
        if let Err(e) = file_checkpointed_write(ctx, &path, input.content.as_bytes()).await {
            return ToolOutput::error(format!("cannot write {}: {e}", path.display()));
        }
        // D-395:写日志凭据——write 是专用写者,写后留痕供跨树围栏吸收。
        record_worktree_write_log(ctx, &input.path, input.content.as_bytes());
        let mut message = format!("wrote {} bytes to {}", input.content.len(), path.display());
        let validation = crate::local_validation::validate_after_write(
            &path,
            &ctx.project_root,
            previous.as_deref(),
            &input.content,
        )
        .await;
        message.push_str(&format!("\n{}", validation.summary));
        let mut display = match previous {
            Some(old) => diff_display(&input.path, &old, &input.content),
            None => serde_json::json!({
                "kind": "create",
                "path": input.path,
                "bytes": input.content.len(),
                "preview": input.content.lines().take(30).collect::<Vec<_>>().join("\n"),
            }),
        };
        if let Some(object) = display.as_object_mut() {
            object.insert("local_validation".into(), validation.display);
        }
        ToolOutput::ok(message).with_display(display)
    }
}

/// 统一 diff 展示(edit/write 共用)。上限截断,防止巨型文件撑爆前端。
pub(crate) fn diff_display(path: &str, old: &str, new: &str) -> serde_json::Value {
    let diff = similar::TextDiff::from_lines(old, new);
    let mut additions = 0usize;
    let mut deletions = 0usize;
    let mut text = String::new();
    let mut lines = Vec::new();
    let mut old_line = 1usize;
    let mut new_line = 1usize;
    let mut truncated = false;
    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            similar::ChangeTag::Insert => {
                additions += 1;
                "+"
            }
            similar::ChangeTag::Delete => {
                deletions += 1;
                "-"
            }
            similar::ChangeTag::Equal => " ",
        };
        let value = change.value().trim_end_matches('\n');
        if text.len() < 24 * 1024 {
            let (kind, old, new) = match change.tag() {
                similar::ChangeTag::Insert => ("add", None, Some(new_line)),
                similar::ChangeTag::Delete => ("del", Some(old_line), None),
                similar::ChangeTag::Equal => ("ctx", Some(old_line), Some(new_line)),
            };
            lines.push(serde_json::json!({
                "kind": kind,
                "old_line": old,
                "new_line": new,
                "text": value,
            }));
        } else {
            truncated = true;
        }
        match change.tag() {
            similar::ChangeTag::Insert => new_line += 1,
            similar::ChangeTag::Delete => old_line += 1,
            similar::ChangeTag::Equal => {
                old_line += 1;
                new_line += 1;
            }
        }
        // 等值上下文只保留短窗口由前端裁剪;此处限制总量。
        if text.len() < 24 * 1024 {
            text.push_str(sign);
            text.push_str(change.value());
            if !change.value().ends_with('\n') {
                text.push('\n');
            }
        }
    }
    if text.len() >= 24 * 1024 {
        truncated = true;
        text.push_str("(diff 截断)\n");
    }
    serde_json::json!({
        "kind": "diff",
        "path": path,
        "language": diff_language(path),
        "diff": text,
        "lines": lines,
        "additions": additions,
        "deletions": deletions,
        "truncated": truncated,
    })
}

fn diff_language(path: &str) -> &'static str {
    match path
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "rs" => "rust",
        "js" | "jsx" => "javascript",
        "ts" | "tsx" => "typescript",
        "json" => "json",
        "toml" => "toml",
        "md" | "markdown" => "markdown",
        "css" => "css",
        "html" | "htm" => "html",
        "py" => "python",
        "ps1" | "sh" | "bash" => "shell",
        _ => "text",
    }
}

#[cfg(test)]
mod tests {
    use super::{diff_display, WriteTool};
    use crate::{BaseComponent, DevProfile};
    use kanzei_core::{AskFuture, AskReply, AskRequest, AskResponse, RunnerConfig};
    use kanzei_harness::{
        rule, ConfigComponent, Harness, KanzeiConfig, ProfileKind, ResolveCtx, ToolCtx,
    };
    use kanzei_llm::{LlmClient, ProxyConfig, ReasoningEffort, Route};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    #[test]
    fn diff_display_exposes_language_counts_and_line_numbers() {
        let display = diff_display(
            "src/main.rs",
            "fn main() {\nold();\n}\n",
            "fn main() {\nnew();\n}\n",
        );
        assert_eq!(display["kind"], "diff");
        assert_eq!(display["language"], "rust");
        assert_eq!(display["additions"], 1);
        assert_eq!(display["deletions"], 1);
        assert_eq!(display["lines"][0]["old_line"], 1);
        assert_eq!(display["lines"][0]["new_line"], 1);
        assert_eq!(display["lines"][1]["kind"], "del");
        assert_eq!(display["lines"][1]["old_line"], 2);
        assert_eq!(display["lines"][2]["kind"], "add");
        assert_eq!(display["lines"][2]["new_line"], 2);
    }

    #[tokio::test]
    async fn permission_path_and_write落点使用同一规范化结果() {
        use kanzei_harness::permission::{Effect, Rule, Ruleset};
        use kanzei_harness::{Tool, ToolCtx};

        let root = std::env::temp_dir().join(format!(
            "kanzei-d050-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let input_path = ".KANZEI/./project/../allowed.md";
        let normalized = kanzei_harness::permission::normalize_resource(input_path);
        assert_eq!(normalized, ".kanzei/allowed.md");

        let deny = Ruleset::new(vec![Rule {
            action: "write".into(),
            resource: "*.kanzei/project/*".into(),
            effect: Effect::Deny,
        }]);
        assert_eq!(
            deny.evaluate(
                "write",
                &kanzei_harness::permission::normalize_resource(".KANZEI\\project\\secret.md")
            ),
            Effect::Deny
        );

        let output = WriteTool
            .execute(
                serde_json::json!({"path": input_path, "content": "落点一致"}),
                &ToolCtx {
                    cwd: root.clone(),
                    project_root: root.clone(),
                    ..Default::default()
                },
            )
            .await;
        assert!(!output.is_error, "{output:?}");
        assert_eq!(
            std::fs::read_to_string(root.join(".kanzei/allowed.md")).unwrap(),
            "落点一致"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn file_checkpointed_write_records_new_file_and_bypasses_unidentified_contexts() {
        use kanzei_harness::Tool;
        use std::path::PathBuf;

        fn temp_root(label: &str) -> PathBuf {
            let root = std::env::temp_dir().join(format!(
                "kz-write-checkpoint-{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&root).unwrap();
            root
        }

        let root = temp_root("identified");
        let ctx = ToolCtx::new(root.clone(), root.clone()).with_identity(
            "tree-key".into(),
            "project-key".into(),
            "run-new-file".into(),
            "process-new-file".into(),
        );
        let output = WriteTool
            .execute(
                serde_json::json!({"path": "created.txt", "content": "created"}),
                &ctx,
            )
            .await;
        assert!(!output.is_error, "{output:?}");
        let db = rusqlite::Connection::open(kanzei_core::store::project_state_path(&root)).unwrap();
        let key = kanzei_core::store::file_checkpoint_path_key(&root.join("created.txt"));
        let (pre_exists, blob, pre_bytes): (i64, Option<String>, i64) = db
            .query_row(
                "SELECT pre_exists, pre_blob, pre_bytes FROM file_checkpoints
                  WHERE run_id = ?1 AND path_key = ?2",
                rusqlite::params!["run-new-file", key],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!((pre_exists, blob, pre_bytes), (0, None, 0));
        assert!(root.join(".kanzei/.write-log").is_dir());
        let logs = crate::write_log::entries_after(&root, 0);
        assert!(
            logs.iter().any(|entry| {
                entry.path == "created.txt"
                    && entry.run_id.as_deref() == Some("run-new-file")
                    && entry.process_id.as_deref() == Some("process-new-file")
            }),
            "identity write log entry should preserve path and identity: {logs:?}"
        );
        drop(db);
        std::fs::remove_dir_all(&root).ok();

        let root_without_run = temp_root("no-run");
        let ctx_without_run = ToolCtx::new(root_without_run.clone(), root_without_run.clone());
        let output = WriteTool
            .execute(
                serde_json::json!({"path": "plain.txt", "content": "plain"}),
                &ctx_without_run,
            )
            .await;
        assert!(!output.is_error, "{output:?}");
        assert!(!root_without_run.join(".kanzei").exists());
        std::fs::remove_dir_all(&root_without_run).ok();

        let root_without_project = temp_root("empty-project-root");
        let ctx_without_project = ToolCtx::new(root_without_project.clone(), PathBuf::new())
            .with_identity(
                "tree-key".into(),
                "".into(),
                "run-empty-project".into(),
                "process-empty-project".into(),
            );
        let output = WriteTool
            .execute(
                serde_json::json!({"path": "plain.txt", "content": "plain"}),
                &ctx_without_project,
            )
            .await;
        assert!(!output.is_error, "{output:?}");
        assert!(!root_without_project.join(".kanzei").exists());
        std::fs::remove_dir_all(root_without_project).ok();
    }

    fn checkpoint_temp_root(label: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kz-write-checkpoint-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        use sha2::Digest;
        sha2::Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    #[tokio::test]
    async fn 子目录cwd绝对路径写入记代码树根与相对树根路径且只开一次库() {
        use kanzei_harness::Tool;

        let root = checkpoint_temp_root("tree-root");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        let sub = root.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let ctx = ToolCtx::new(sub.clone(), root.clone()).with_identity(
            "tree-key".into(),
            "project-key".into(),
            "run-tree-root".into(),
            "process-tree-root".into(),
        );
        let absolute = sub.join("x.txt");
        let output = WriteTool
            .execute(
                serde_json::json!({"path": absolute.to_string_lossy(), "content": "x"}),
                &ctx,
            )
            .await;
        assert!(!output.is_error, "{output:?}");
        assert_eq!(std::fs::read_to_string(&absolute).unwrap(), "x");

        let state = kanzei_core::store::project_state_path(&root);
        // 审计 18:前像与后像共用一次 open。
        assert_eq!(kanzei_core::store::store_open_count(&state), 1);
        let db = rusqlite::Connection::open(&state).unwrap();
        let (tree_root, rel_path, pre_exists): (String, String, i64) = db
            .query_row(
                "SELECT tree_root, rel_path, pre_exists FROM file_checkpoints WHERE run_id = ?1",
                ["run-tree-root"],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(tree_root, root.to_string_lossy());
        assert_eq!(rel_path, "sub/x.txt");
        assert_eq!(pre_exists, 0);
        drop(db);
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn 前像采集失败后连写两次只留哨兵行不补采中间态() {
        use kanzei_harness::Tool;

        let root = checkpoint_temp_root("sentinel");
        // blob 目录位置被普通文件占住:state.db 可写,只有前像 blob 落不下。
        let artifacts = root.join(".kanzei/artifacts");
        std::fs::create_dir_all(&artifacts).unwrap();
        std::fs::write(artifacts.join("checkpoints"), b"not a directory").unwrap();
        std::fs::write(root.join("target.txt"), "original").unwrap();
        let ctx = ToolCtx::new(root.clone(), root.clone()).with_identity(
            "tree-key".into(),
            "project-key".into(),
            "run-sentinel".into(),
            "process-sentinel".into(),
        );
        for content in ["first write", "second write"] {
            let output = WriteTool
                .execute(
                    serde_json::json!({"path": "target.txt", "content": content}),
                    &ctx,
                )
                .await;
            assert!(!output.is_error, "{output:?}");
        }

        let db = rusqlite::Connection::open(kanzei_core::store::project_state_path(&root)).unwrap();
        let rows: Vec<(i64, Option<String>, i64, Option<String>)> = db
            .prepare(
                "SELECT pre_exists, pre_blob, pre_bytes, post_hash
                   FROM file_checkpoints WHERE run_id = ?1",
            )
            .unwrap()
            .query_map(["run-sentinel"], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            rows,
            vec![(
                1,
                None,
                kanzei_core::store::FILE_CHECKPOINT_UNKNOWN_PRE_BYTES,
                Some(sha256_hex(b"second write"))
            )],
            "第一次写入的内容绝不能被记成前像"
        );
        drop(db);
        std::fs::remove_dir_all(&root).ok();
    }

    type CheckpointRow = (i64, Option<String>, i64, Option<String>);

    fn checkpoint_rows(root: &std::path::Path, run_id: &str) -> Vec<CheckpointRow> {
        let db = rusqlite::Connection::open(kanzei_core::store::project_state_path(root)).unwrap();
        let rows = db
            .prepare(
                "SELECT pre_exists, pre_blob, pre_bytes, post_hash
                   FROM file_checkpoints WHERE run_id = ?1",
            )
            .unwrap()
            .query_map([run_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        rows
    }

    /// 审计 19(1):写前 SessionStore::open 失败(库忙等)时,落盘后再 open 一次只为占
    /// 哨兵行;同 run 之后的触碰不得把这次写出的中间态补采成前像。
    #[tokio::test]
    async fn 写前打开库失败时落盘后重开一次占哨兵行且不补采中间态() {
        use kanzei_core::store::{SessionStore, StoreError};
        use kanzei_harness::Tool;
        use std::cell::Cell;

        let root = checkpoint_temp_root("reopen");
        let path = root.join("target.txt");
        std::fs::write(&path, "original").unwrap();
        let attempts = Cell::new(0_u32);
        let first_open_fails = |state: &std::path::Path| {
            attempts.set(attempts.get() + 1);
            if attempts.get() == 1 {
                Err(StoreError::Io(std::io::Error::other(
                    "injected: state.db busy",
                )))
            } else {
                SessionStore::open(state)
            }
        };
        super::file_checkpointed_write_blocking_with(
            first_open_fails,
            &root,
            &root,
            "run-reopen",
            Some("process-reopen"),
            &path,
            b"first write",
        )
        .unwrap();
        assert_eq!(attempts.get(), 2, "写前失败后应在落盘后重开恰好一次");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "first write");
        assert_eq!(
            checkpoint_rows(&root, "run-reopen"),
            vec![(
                1,
                None,
                kanzei_core::store::FILE_CHECKPOINT_UNKNOWN_PRE_BYTES,
                Some(sha256_hex(b"first write"))
            )]
        );

        // 库恢复后同 run 再写(生产入口):哨兵行挡住补采,"first write" 不会被记成前像。
        let ctx = ToolCtx::new(root.clone(), root.clone()).with_identity(
            "tree-key".into(),
            "project-key".into(),
            "run-reopen".into(),
            "process-reopen".into(),
        );
        let output = WriteTool
            .execute(
                serde_json::json!({"path": "target.txt", "content": "second write"}),
                &ctx,
            )
            .await;
        assert!(!output.is_error, "{output:?}");
        assert_eq!(
            checkpoint_rows(&root, "run-reopen"),
            vec![(
                1,
                None,
                kanzei_core::store::FILE_CHECKPOINT_UNKNOWN_PRE_BYTES,
                Some(sha256_hex(b"second write"))
            )],
            "第一次写入的内容绝不能被记成前像"
        );
        assert!(!root.join(".kanzei/artifacts/checkpoints").exists());
        std::fs::remove_dir_all(&root).ok();

        // 两次都打不开:写入照样成功,只是不留行(已知边界,见模块头)。
        let unavailable = checkpoint_temp_root("reopen-unavailable");
        let unavailable_path = unavailable.join("target.txt");
        let attempts = Cell::new(0_u32);
        super::file_checkpointed_write_blocking_with(
            |_state: &std::path::Path| {
                attempts.set(attempts.get() + 1);
                Err(StoreError::Io(std::io::Error::other(
                    "injected: unavailable",
                )))
            },
            &unavailable,
            &unavailable,
            "run-unavailable",
            None,
            &unavailable_path,
            b"written",
        )
        .unwrap();
        assert_eq!(attempts.get(), 2);
        assert_eq!(
            std::fs::read_to_string(&unavailable_path).unwrap(),
            "written"
        );
        assert!(!kanzei_core::store::project_state_path(&unavailable).exists());
        std::fs::remove_dir_all(&unavailable).ok();
    }

    /// 写工具的 normalize_resource 在 Windows 上把入参转小写:相对入参得到「原大小写
    /// cwd + 小写尾部」,绝对入参得到整串小写。同一文件两种写法必须记成同一个 rel_path。
    #[tokio::test]
    async fn 相对与绝对入参写同一文件记同一rel_path() {
        use kanzei_harness::Tool;

        let root = checkpoint_temp_root("rel-consistency");
        std::fs::create_dir_all(root.join("Sub")).unwrap();
        let absolute = root.join("Sub").join("X.txt");
        for (run_id, input) in [
            ("run-relative-input", "Sub/X.txt".to_string()),
            (
                "run-absolute-input",
                absolute.to_string_lossy().into_owned(),
            ),
        ] {
            let ctx = ToolCtx::new(root.clone(), root.clone()).with_identity(
                "tree-key".into(),
                "project-key".into(),
                run_id.into(),
                "process-rel".into(),
            );
            let output = WriteTool
                .execute(serde_json::json!({"path": input, "content": run_id}), &ctx)
                .await;
            assert!(!output.is_error, "{output:?}");
        }

        let db = rusqlite::Connection::open(kanzei_core::store::project_state_path(&root)).unwrap();
        let rel_path = |run_id: &str| -> String {
            db.query_row(
                "SELECT rel_path FROM file_checkpoints WHERE run_id = ?1",
                [run_id],
                |row| row.get(0),
            )
            .unwrap()
        };
        let from_relative = rel_path("run-relative-input");
        let from_absolute = rel_path("run-absolute-input");
        assert_eq!(from_relative, from_absolute);
        let expected = if cfg!(windows) {
            "sub/x.txt"
        } else {
            "Sub/X.txt"
        };
        assert_eq!(from_relative, expected);
        drop(db);
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn runner_hard_deny_blocks_real_write_tool_before_filesystem_side_effect() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let root = std::env::temp_dir().join(format!(
            "kanzei-d050-runner-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let input_path = if cfg!(windows) {
            r".KANZEI\project\requirements.md"
        } else {
            r".kanzei\project\requirements.md"
        };
        let call_input = serde_json::json!({
            "path": input_path,
            "content": "must not be written"
        });
        let first_response = serde_json::json!({
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_d050",
                        "type": "function",
                        "function": {
                            "name": "write",
                            "arguments": call_input.to_string()
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1}
        });
        let second_response = serde_json::json!({
            "choices": [{
                "index": 0,
                "delta": {"content": "权限拒绝"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1}
        });
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for response in [first_response, second_response] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut chunk = [0_u8; 4096];
                let header_end = loop {
                    let count = stream.read(&mut chunk).await.unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&chunk[..count]);
                    if let Some(position) =
                        request.windows(4).position(|window| window == b"\r\n\r\n")
                    {
                        break position + 4;
                    }
                };
                let content_length = String::from_utf8_lossy(&request[..header_end])
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(0);
                while request.len() < header_end + content_length {
                    let count = stream.read(&mut chunk).await.unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&chunk[..count]);
                }
                let body = format!("data: {}\n\ndata: [DONE]\n\n", response);
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream.write_all(head.as_bytes()).await.unwrap();
                stream.write_all(body.as_bytes()).await.unwrap();
            }
        });

        let mut config = KanzeiConfig::default();
        config.permissions.rules.push(rule(
            "write",
            "*.kanzei/project/*",
            kanzei_harness::Effect::Ask,
        ));
        let resolve_ctx = ResolveCtx {
            profile: ProfileKind::Dev,
            cwd: root.clone(),
            project_root: root.clone(),
            config: Arc::new(config),
        };
        let mut harness = Harness::default();
        harness
            .add(BaseComponent)
            .add(DevProfile)
            .add(ConfigComponent);
        let snapshot = harness.resolve(&resolve_ctx).unwrap();
        let agent = snapshot.select_agent(None).unwrap();
        let client = LlmClient::new(&ProxyConfig::Disabled).unwrap();
        let route = Route::openai_at(&format!("http://{address}/v1"), None);
        let runner_config = RunnerConfig {
            intensity: kanzei_harness::HarnessIntensity::Autonomous,
            model: "mock".into(),
            max_tokens: 128,
            reasoning: ReasoningEffort::Off,
            service_tier: None,
            context_limit: None,
            limits: Default::default(),
            recall: None,
            execution_policy: kanzei_harness::orchestration::ExecutionPolicy::Default,
            ask_policy: kanzei_core::AskPolicy::Interactive,
            halt: None,
        };
        let tool_ctx = ToolCtx {
            cwd: root.clone(),
            project_root: root.clone(),
            ..Default::default()
        };
        let mut on_event = |_event| {};
        let ask_count = Arc::new(AtomicUsize::new(0));
        let ask_count_for_callback = Arc::clone(&ask_count);
        let mut ask = move |_request: AskRequest| -> AskFuture {
            ask_count_for_callback.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { AskResponse::Permission(AskReply::Deny) })
        };

        let summary = kanzei_core::run_once(
            &client,
            &route,
            snapshot.as_ref(),
            agent,
            &runner_config,
            &tool_ctx,
            "执行写入",
            None,
            &[],
            None,
            // R-246:测试不持有 LineRuntime。
            None,
            &mut on_event,
            &mut ask,
        )
        .await
        .unwrap();

        assert_eq!(ask_count.load(Ordering::SeqCst), 0);
        assert!(summary.messages.iter().flat_map(|message| &message.parts).any(
            |part| matches!(part, kanzei_llm::Part::ToolResult { content, is_error: true, .. } if content.contains("permission denied"))
        ));
        assert!(!root.join(".kanzei/project/requirements.md").exists());
        server.await.unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn diff_display_marks_large_output_as_truncated() {
        let old = "a\n".repeat(20_000);
        let new = "b\n".repeat(20_000);
        let display = diff_display("notes.txt", &old, &new);
        assert_eq!(display["language"], "text");
        assert_eq!(display["truncated"], true);
        assert!(display["lines"].as_array().unwrap().len() < 40_000);
    }
}
