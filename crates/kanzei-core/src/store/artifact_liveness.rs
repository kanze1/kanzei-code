//! Artifact publication spans writing the blob and committing its database reference.
//! GC takes the same project's exclusive lock before reading or deleting candidates.
//! FileLock stays on its blocking worker; async callers own only its release sender.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use kanzei_base::atomic_file::{self, FileLock};

fn target(project_root: &Path) -> PathBuf {
    project_root.join(".kanzei/artifacts/tool-results-liveness")
}

/// Synchronous publication keeps this thread-owned guard entirely before any await.
pub fn lock_publication(project_root: &Path) -> io::Result<FileLock> {
    atomic_file::lock_shared(&target(project_root))
}

/// Keeps a pending artifact alive until its reference is committed or publication stops.
/// Dropping this sender wakes the worker and releases its thread-owned FileLock.
pub struct PublicationGuard {
    _release: std::sync::mpsc::Sender<()>,
}

/// This shared lifecycle lock is separate from the short quota lock. Concurrent
/// publishers are allowed; cleanup reports busy while any publisher is active.
pub async fn acquire_publication(project_root: &Path) -> io::Result<PublicationGuard> {
    let root = project_root.to_path_buf();
    let (release, released) = std::sync::mpsc::channel::<()>();
    let (ready, acquired) = tokio::sync::oneshot::channel();
    tokio::task::spawn_blocking(move || match lock_publication(&root) {
        Ok(_lock) => {
            if ready.send(Ok(())).is_ok() {
                let _ = released.recv();
            }
        }
        Err(error) => {
            let _ = ready.send(Err(error));
        }
    });
    acquired.await.map_err(|error| {
        io::Error::other(format!("artifact publication worker failed: {error}"))
    })??;
    Ok(PublicationGuard { _release: release })
}

/// Cleanup is explicit and must report a live publisher instead of waiting for an LLM.
pub(super) fn lock_cleanup(project_root: &Path) -> io::Result<FileLock> {
    atomic_file::try_lock_exclusive(&target(project_root), Duration::ZERO)?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::WouldBlock,
            "上下文原文仍在归档或压缩中，请完成后再整理或删除对话",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DigestModel, SessionStore};
    use futures::FutureExt;
    use kanzei_llm::{LlmClient, Message, ProxyConfig, Route};
    use serde_json::json;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

    fn root(tag: &str) -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kz-artifact-liveness-{tag}-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn await_worker_release(root: &Path) {
        let _settled = atomic_file::try_lock_exclusive(&target(root), Duration::from_secs(3))
            .unwrap()
            .expect("publication worker must release its lock after the async owner ends");
    }

    #[tokio::test]
    async fn shared_publishers_and_cancelled_acquisition_release_their_worker_locks() {
        let root = root("lifecycle");
        let first = acquire_publication(&root).await.unwrap();
        let second = acquire_publication(&root).await.unwrap();
        assert_eq!(
            lock_cleanup(&root).err().unwrap().kind(),
            io::ErrorKind::WouldBlock
        );
        drop(first);
        assert!(
            lock_cleanup(&root).is_err(),
            "the last publisher still owns the shared lock"
        );
        drop(second);
        await_worker_release(&root);
        {
            let _cleanup = lock_cleanup(&root).unwrap();
            // Poll once while acquisition is guaranteed to be pending, then cancel it.
            assert!(acquire_publication(&root).now_or_never().is_none());
        }
        let next = acquire_publication(&root).await.unwrap();
        drop(next);
        await_worker_release(&root);
        let (ready, acquired) = tokio::sync::oneshot::channel();
        let task_root = root.clone();
        let owner = tokio::spawn(async move {
            let _publication = acquire_publication(&task_root).await.unwrap();
            ready.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        acquired.await.unwrap();
        assert!(lock_cleanup(&root).is_err());
        owner.abort();
        assert!(owner.await.unwrap_err().is_cancelled());
        await_worker_release(&root);
        std::fs::remove_dir_all(root).unwrap();
    }

    async fn compaction_gc_case(changed_source: bool) {
        let root = root(if changed_source { "stale" } else { "commit" });
        let state_path = crate::project_state_path(&root);
        let session = "ses-compaction-gc";
        let mut messages = vec![Message::user_text("任务定义：保留协作历史并接续当前工作")];
        for index in 0..60 {
            messages.push(Message::user_text(format!(
                "中段第 {index} 条 {}",
                "x".repeat(400)
            )));
        }
        messages.push(Message::user_text("最近工作：已执行验证"));
        messages.push(Message::user_text("当前请求：继续处理通用系统问题"));
        let before = crate::estimate_conversation_tokens(&messages);
        let sequence = {
            let store = SessionStore::open(&state_path).unwrap();
            store
                .create_session(session, root.to_str().unwrap(), None)
                .unwrap();
            store
                .append_event(
                    session,
                    "conversation.updated",
                    &json!({"messages": messages}),
                )
                .unwrap()
                .sequence
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (entered, request_seen) = tokio::sync::oneshot::channel();
        let (resume, resumed) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            {
                let mut reader = tokio::io::BufReader::new(&mut socket);
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    assert!(reader.read_line(&mut line).await.unwrap() > 0);
                    if line == "\r\n" {
                        break;
                    }
                    if let Some((name, value)) = line.split_once(':') {
                        if name.eq_ignore_ascii_case("content-length") {
                            length = value.trim().parse::<usize>().unwrap();
                        }
                    }
                }
                reader.read_exact(&mut vec![0; length]).await.unwrap();
            }
            entered.send(()).unwrap();
            resumed.await.unwrap();
            let summary = "任务仍是保留协作历史并接续当前工作。此前已经逐项核对需求和运行状态，完成必要的验证，下一步继续处理通用系统问题。已确定的约束应继续保留，不能恢复过时状态或重复已经完成的工作。";
            let body = format!(
                "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                json!({"choices":[{"index":0,"delta":{"content":summary}}]}),
                json!({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]})
            );
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        let worker_root = root.clone();
        let worker_state = state_path.clone();
        let compaction = tokio::spawn(async move {
            let _publication = acquire_publication(&worker_root).await.unwrap();
            let model = DigestModel {
                route: Route::openai_at(&format!("http://{address}"), None),
                model: "mock-compaction".into(),
                service_tier: None,
                focus: None,
                archive_root: Some(worker_root),
            };
            let client = LlmClient::new(&ProxyConfig::Disabled).unwrap();
            let dropped = crate::compact_conversation_with_model(
                &client,
                Some(&model),
                &mut messages,
                before.max(4000),
                &mut Vec::new(),
                0.2,
            )
            .await;
            assert!(
                dropped > 0,
                "the real compactor must produce an archived replacement"
            );
            let store = SessionStore::open(&worker_state).unwrap();
            store.append_compaction_transaction_checked(
                session,
                "manual-gc-regression",
                &json!({"manual": true}),
                &json!(messages),
                Some(sequence),
            )
        });
        tokio::time::timeout(Duration::from_secs(10), request_seen)
            .await
            .unwrap()
            .unwrap();
        let store = SessionStore::open_for_explicit_cleanup(&state_path).unwrap();
        let plan = store.artifact_cleanup_plan(&root).unwrap();
        assert_eq!(
            plan.unreferenced_artifact_files, 1,
            "the archive exists before the gated LLM returns"
        );
        let archive = root.join(&plan.unreferenced[0].relative_path);
        assert!(
            store.cleanup_storage(&root).is_err(),
            "GC must reject while the archive has no committed reference"
        );
        assert!(archive.exists());
        assert!(store.delete_session(session, &root).is_err());
        assert!(store.get_session(session).unwrap().is_some());
        if changed_source {
            store
                .append_event(session, "conversation.reset", &json!({}))
                .unwrap();
        }
        resume.send(()).unwrap();
        let result = tokio::time::timeout(Duration::from_secs(10), compaction)
            .await
            .unwrap()
            .unwrap();
        server.await.unwrap();
        await_worker_release(&root);
        if changed_source {
            assert!(result.is_err());
            assert!(store
                .list_events_by_type(session, 0, "surface_replaced")
                .unwrap()
                .is_empty());
            let cleaned = store.cleanup_storage(&root).unwrap();
            assert_eq!(cleaned.deleted_artifacts.len(), 1);
            assert!(!archive.exists());
        } else {
            result.unwrap();
            assert_eq!(
                store
                    .artifact_cleanup_plan(&root)
                    .unwrap()
                    .referenced_artifact_files,
                1
            );
            let cleaned = store.cleanup_storage(&root).unwrap();
            assert!(cleaned.deleted_artifacts.is_empty());
            assert!(
                archive.exists(),
                "a committed compaction pointer must still be readable"
            );
        }
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn gated_llm_compaction_archive_survives_gc_until_reference_commit() {
        compaction_gc_case(false).await;
    }

    #[tokio::test]
    async fn failed_checked_compaction_releases_publication_and_preserves_history() {
        compaction_gc_case(true).await;
    }
}
