//! Observe origin evidence once, before this run's scouts, claim and main writes.
//! Later dirty/untracked state must never be substituted for the saved baseline.

use kanzei_core::SessionStore;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_STATUS_BYTES: usize = 16 * 1024;
const MAX_PATHS: usize = 64;
const MAX_PATH_CHARS: usize = 320;
const MAX_MODEL_CHARS: usize = 3_200;

pub(crate) const ORIGIN_RULES: &str = "来源判断：现时 ?? 只表示现在未跟踪，不证明开跑前就有用户稿。覆盖/丢失事故必须对照本 run 起点、对应 HEAD/起点改动和该 run 成功 write/edit/insert 事实；路径存在、执行方或复核方自述都不能单独证明用户所有权或覆盖。起点命令失败、截断或来源证据不足时说未知，先回读证据，不编造事故。先明确本任务范围、验收和交接；既定验证已通过后按用户要求收口，新增工作只针对实际失败或明确未满足的验收。";

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct RunBaseline {
    pub(crate) schema_version: u32,
    pub(crate) run_id: String,
    pub(crate) captured_at_ms: u64,
    pub(crate) finished_at_ms: u64,
    pub(crate) cwd: String,
    pub(crate) head: Option<String>,
    pub(crate) head_stable: bool,
    pub(crate) status: String,
    pub(crate) dirty: Vec<DirtyPath>,
    pub(crate) untracked: Vec<String>,
    pub(crate) truncated: bool,
    pub(crate) errors: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct DirtyPath {
    path: String,
    index: String,
    worktree: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    original_path: Option<String>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

pub(crate) async fn capture(cwd: &Path, run_id: &str) -> RunBaseline {
    capture_with_limit(cwd, run_id, MAX_STATUS_BYTES).await
}

/// Establish one run's origin and persist it before any scout, claim or main write.
/// Return the same identity/context for later typed events and the pipeline reviewer.
pub(crate) async fn prepare(
    profile: kanzei_harness::ProfileKind,
    cwd: &Path,
    store: &mut SessionStore,
    session_id: &str,
    system: &mut String,
) -> anyhow::Result<(String, String)> {
    let run_id = format!(
        "run_{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    );
    if crate::general_chat::is_general_root(cwd) {
        store.append_event(
            session_id,
            "run.general",
            &serde_json::json!({"run_id": run_id, "scope": "general"}),
        )?;
        return Ok((run_id, String::new()));
    }
    let baseline = if profile == kanzei_harness::ProfileKind::Isolated {
        RunBaseline {
            schema_version: 1,
            run_id: run_id.clone(),
            captured_at_ms: now_ms(),
            finished_at_ms: now_ms(),
            cwd: cwd.to_string_lossy().into_owned(),
            head: None,
            head_stable: false,
            status: "not_observed_isolated".into(),
            dirty: vec![],
            untracked: vec![],
            truncated: false,
            errors: vec!["隔离档位不执行宿主 Git；起点版本与改动状态未知".into()],
        }
    } else {
        capture(cwd, &run_id).await
    };
    baseline.persist(store, session_id)?;
    let context = baseline.model_context();
    system.push_str(&context);
    Ok((run_id, context))
}

async fn capture_with_limit(cwd: &Path, run_id: &str, limit: usize) -> RunBaseline {
    let captured_at_ms = now_ms();
    let directory = cwd.to_path_buf();
    let observed = if kanzei_tools::project_state::is_repository_root(cwd) {
        tokio::task::spawn_blocking(move || kanzei_tools::git_observe::status(&directory))
            .await
            .map_err(|error| error.to_string())
            .and_then(|result| result)
    } else {
        Err("代码树根没有独立 Git 仓库，未读取上级仓库".into())
    };
    let mut errors = Vec::new();
    let (head, head_stable, mut raw, failed) = match observed {
        Ok(observation) => {
            let head = observation.head_before.as_ref().ok().cloned();
            let stable = head.is_some() && observation.head_before == observation.head_after;
            errors.extend(
                [observation.head_before.err(), observation.head_after.err()]
                    .into_iter()
                    .flatten(),
            );
            if head.is_some() && !stable {
                errors.push("采样期间 HEAD 变化或末次读取失败，起点版本未知".into());
            }
            (head, stable, observation.porcelain, false)
        }
        Err(error) => {
            errors.push(error);
            (None, false, vec![], true)
        }
    };
    let bytes_truncated = raw.len() > limit;
    raw.truncate(limit);
    let (dirty, untracked, paths_truncated) = parse_status(&raw);
    let truncated = bytes_truncated || paths_truncated;
    RunBaseline {
        schema_version: 1,
        run_id: run_id.into(),
        captured_at_ms,
        finished_at_ms: now_ms(),
        cwd: cwd.display().to_string(),
        head,
        head_stable,
        status: if failed {
            "unknown"
        } else if truncated {
            "partial"
        } else {
            "known"
        }
        .into(),
        dirty,
        untracked,
        truncated,
        errors,
    }
}

fn parse_status(raw: &[u8]) -> (Vec<DirtyPath>, Vec<String>, bool) {
    let mut dirty = Vec::new();
    let mut untracked = Vec::new();
    let mut truncated = !raw.is_empty() && raw.last() != Some(&0);
    let mut records = raw.split(|byte| *byte == 0);
    while let Some(record) = records.next() {
        if record.len() < 4 || record[2] != b' ' {
            continue;
        }
        let path = String::from_utf8_lossy(&record[3..]);
        let original =
            if record[0] == b'R' || record[0] == b'C' || record[1] == b'R' || record[1] == b'C' {
                records
                    .next()
                    .filter(|original| !original.is_empty())
                    .map(|original| String::from_utf8_lossy(original).to_string())
            } else {
                None
            };
        if dirty.len() + untracked.len() >= MAX_PATHS {
            truncated = true;
            break;
        }
        truncated |= path.chars().count() > MAX_PATH_CHARS
            || original
                .as_ref()
                .is_some_and(|path| path.chars().count() > MAX_PATH_CHARS);
        let path = path.chars().take(MAX_PATH_CHARS).collect();
        if record[..2] == *b"??" {
            untracked.push(path);
        } else {
            dirty.push(DirtyPath {
                path,
                index: (record[0] as char).to_string(),
                worktree: (record[1] as char).to_string(),
                original_path: original.map(|path| path.chars().take(MAX_PATH_CHARS).collect()),
            });
        }
    }
    (dirty, untracked, truncated)
}

impl RunBaseline {
    pub(crate) fn persist(
        &self,
        store: &SessionStore,
        session_id: &str,
    ) -> Result<(), kanzei_core::StoreError> {
        store.append_event(session_id, "run.baseline", &serde_json::to_value(self)?)?;
        Ok(())
    }

    pub(crate) fn model_context(&self) -> String {
        let evidence = serde_json::to_string(self).unwrap_or_default();
        let clipped = evidence.chars().count() > MAX_MODEL_CHARS;
        let preview: String = evidence.chars().take(MAX_MODEL_CHARS).collect();
        format!("\n\n[本 run 起点来源证据，仅为开跑时观测]\n{preview}{}\n完整证据：项目 state.db 的 run.baseline，run_id={}。\n{ORIGIN_RULES}\n", if clipped { "\n[模型预览已截断，未显示的路径/状态未知；须回读完整事件。]" } else { "" }, self.run_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "kz_baseline_{tag}_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }
    fn git(root: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    #[tokio::test]
    async fn isolated_start_does_not_execute_a_repository_fsmonitor_hook() {
        let root = fixture("isolated-hook");
        let state = fixture("isolated-hook-state");
        git(&root, &["init", "-q"]);
        std::fs::write(root.join("tracked.txt"), "fixture").unwrap();
        git(&root, &["add", "tracked.txt"]);
        let marker = state.join("host-hook-ran.txt");
        let hook = root.join("fsmonitor-hook.sh");
        let quoted = marker
            .to_string_lossy()
            .replace('\\', "/")
            .replace('\'', "'\\''");
        std::fs::write(
            &hook,
            format!("#!/bin/sh\nprintf CONTROL > '{quoted}'\nprintf 'token\\0'\n"),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        git(
            &root,
            &[
                "config",
                "core.fsmonitor",
                &hook.to_string_lossy().replace('\\', "/"),
            ],
        );
        git(&root, &["status", "--short"]);
        assert!(
            marker.is_file(),
            "the actual host Git observation executes this owned hook"
        );
        std::fs::remove_file(&marker).unwrap();
        let mut store = SessionStore::open(&state.join("state.db")).unwrap();
        store
            .create_session("isolated-start", &root.to_string_lossy(), None)
            .unwrap();
        let (_, context) = prepare(
            kanzei_harness::ProfileKind::Isolated,
            &root,
            &mut store,
            "isolated-start",
            &mut String::new(),
        )
        .await
        .unwrap();
        assert!(
            !marker.exists(),
            "isolated startup must not execute any host Git observation"
        );
        assert!(context.contains("not_observed_isolated"));
        let events = store.list_events("isolated-start", 0).unwrap();
        assert!(events
            .iter()
            .any(|event| event.event_type == "run.baseline" && event.payload["head"].is_null()));
    }

    #[tokio::test]
    async fn prepared_run_binds_saved_origin_and_main_context_before_later_writes() {
        let root = fixture("prepare");
        let state_root = fixture("prepare-state");
        git(&root, &["init", "-q"]);
        std::fs::write(root.join("draft.md"), "before this run").unwrap();
        let mut store = SessionStore::open(&state_root.join("state.db")).unwrap();
        store
            .create_session("session-prepare", &root.display().to_string(), None)
            .unwrap();
        let mut system = "main agent".to_string();
        let (run_id, context) = prepare(
            kanzei_harness::ProfileKind::Dev,
            &root,
            &mut store,
            "session-prepare",
            &mut system,
        )
        .await
        .unwrap();
        let events = store.list_events("session-prepare", 0).unwrap();
        let origins = events
            .iter()
            .filter(|event| event.event_type == "run.baseline")
            .collect::<Vec<_>>();
        assert_eq!(origins.len(), 1);
        assert_eq!(origins[0].payload["run_id"], run_id);
        assert_eq!(
            origins[0].payload["cwd"],
            kanzei_tools::path_form::simplify(&root)
                .display()
                .to_string()
        );
        assert_eq!(
            origins[0].payload["untracked"],
            serde_json::json!(["draft.md"])
        );
        assert!(context.contains(&run_id));
        assert_eq!(system, format!("main agent{context}"));
        std::fs::write(root.join("later.md"), "created later").unwrap();
        assert!(!origins[0].payload.to_string().contains("later.md"));
        drop(store);
        std::fs::remove_dir_all(root).ok();
        std::fs::remove_dir_all(state_root).ok();
    }
    #[tokio::test]
    async fn saved_origin_survives_later_generated_untracked_files_and_reopen() {
        let root = fixture("origin");
        git(&root, &["init", "-q"]);
        std::fs::write(root.join("tracked.txt"), "initial").unwrap();
        git(&root, &["add", "tracked.txt"]);
        git(
            &root,
            &[
                "-c",
                "user.name=baseline",
                "-c",
                "user.email=baseline@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-qm",
                "initial",
            ],
        );
        std::fs::write(root.join("tracked.txt"), "dirty before run").unwrap();
        std::fs::write(root.join("user draft.md"), "existing draft").unwrap();
        let baseline = capture(&root, "run-origin").await;
        assert_eq!(baseline.status, "known");
        assert!(baseline.head_stable);
        assert_eq!(baseline.untracked, ["user draft.md"]);
        assert_eq!(baseline.dirty[0].path, "tracked.txt");
        let db = root.join("state.db");
        let store = SessionStore::open(&db).unwrap();
        store
            .create_session("s", &root.display().to_string(), None)
            .unwrap();
        baseline.persist(&store, "s").unwrap();
        std::fs::create_dir(root.join("docs")).unwrap();
        std::fs::write(root.join("docs/generated.md"), "created by this run").unwrap();
        drop(store);
        let store = SessionStore::open_read_only(&db).unwrap();
        let events = store.list_events("s", 0).unwrap();
        let saved = events
            .iter()
            .find(|event| event.event_type == "run.baseline")
            .unwrap();
        assert_eq!(saved.payload["run_id"], "run-origin");
        assert_eq!(
            saved.payload["untracked"],
            serde_json::json!(["user draft.md"])
        );
        assert!(!saved.payload.to_string().contains("generated.md"));
        assert!(capture(&root, "later-observation")
            .await
            .untracked
            .iter()
            .any(|path| path == "docs/generated.md"));
        drop(store);
        std::fs::remove_dir_all(root).ok();
    }
    #[tokio::test]
    async fn failed_and_truncated_git_status_never_claim_a_clean_origin() {
        let root = fixture("unknown");
        let unavailable = capture(&root, "no-repo").await;
        assert_eq!(unavailable.status, "unknown");
        assert!(!unavailable.errors.is_empty());
        git(&root, &["init", "-q"]);
        std::fs::write(root.join("a-very-long-untracked-filename.md"), "draft").unwrap();
        let limited = capture_with_limit(&root, "limited", 8).await;
        assert_eq!(limited.status, "partial");
        assert!(limited.truncated);
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn a_nested_non_repository_never_borrows_its_parents_head_or_changes() {
        let root = fixture("nested-root");
        git(&root, &["init", "-q"]);
        std::fs::write(root.join("parent.txt"), "parent evidence").unwrap();
        git(&root, &["add", "."]);
        git(
            &root,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@localhost",
                "commit",
                "-qm",
                "parent",
            ],
        );
        let nested = root.join("nested");
        std::fs::create_dir(&nested).unwrap();
        let facts = capture(&nested, "nested-run").await;
        assert!(facts.head.is_none());
        assert!(facts.dirty.is_empty());
        assert!(facts.untracked.is_empty());
        assert_eq!(facts.status, "unknown");
        assert!(facts
            .errors
            .iter()
            .all(|error| error.contains("未读取上级仓库")));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn porcelain_rename_and_unicode_are_preserved_with_a_bounded_preview() {
        let (dirty, untracked, clipped) =
            parse_status("R  新 名.md\0旧 名.md\0?? 草稿.md\0".as_bytes());
        assert!(!clipped);
        assert_eq!(dirty[0].original_path.as_deref(), Some("旧 名.md"));
        assert_eq!(untracked, ["草稿.md"]);
        let (_, paths, clipped) =
            parse_status(format!("?? {}\0", "x".repeat(MAX_PATH_CHARS + 1)).as_bytes());
        assert!(clipped);
        assert_eq!(paths[0].chars().count(), MAX_PATH_CHARS);
    }
}
