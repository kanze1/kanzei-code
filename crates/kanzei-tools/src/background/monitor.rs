//! Bounded output subscriptions deliver updates to the subscribing actor.
use super::*;
use kanzei_harness::{AsyncMailbox, AsyncNotice};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{atomic::AtomicU64, LazyLock},
    time::Duration,
};

struct Monitor {
    process: String,
    owner: String,
    root: String,
    pattern: Option<String>,
    interval: u64,
    active: AtomicBool,
    mailbox: AsyncMailbox,
}
static MONITORS: LazyLock<Mutex<HashMap<String, Arc<Monitor>>>> = LazyLock::new(Mutex::default);
static SEQ: AtomicU64 = AtomicU64::new(1);
fn key(root: &std::path::Path, owner: &str, id: &str) -> String {
    format!("{}|{owner}|{id}", crate::worktree::worktree_key(root))
}
pub fn subscriptions(root: &std::path::Path, owner: &str) -> Vec<Value> {
    let root = crate::worktree::worktree_key(root);
    MONITORS
        .lock()
        .unwrap()
        .values()
        .filter(|m| {
            m.root == root
                && m.owner == owner
                && m.active.load(Ordering::SeqCst)
                && !m.mailbox.is_closed()
        })
        .map(|m| json!({"id":m.process,"pattern":m.pattern,"interval_secs":m.interval}))
        .collect()
}
pub fn unsubscribe(root: &std::path::Path, owner: &str, id: &str) -> bool {
    MONITORS
        .lock()
        .unwrap()
        .remove(&key(root, owner, id))
        .is_some_and(|m| m.active.swap(false, Ordering::SeqCst))
}
pub fn subscribe(
    root: &std::path::Path,
    owner: &str,
    id: &str,
    mailbox: AsyncMailbox,
    pattern: Option<&str>,
    interval: Option<u64>,
) -> Result<Value, String> {
    let process = get(id).ok_or("后台进程不存在")?;
    if crate::worktree::worktree_key(std::path::Path::new(&process.project_root))
        != crate::worktree::worktree_key(root)
    {
        return Err("后台进程不属于当前项目".into());
    }
    if !process.is_running() {
        return Err("进程已结束，可直接查看输出".into());
    }
    let regex = pattern
        .map(regex::Regex::new)
        .transpose()
        .map_err(|e| format!("订阅过滤条件无效：{e}"))?;
    let interval = interval.unwrap_or(5).clamp(2, 60);
    let monitor = Arc::new(Monitor {
        process: id.into(),
        owner: owner.into(),
        root: crate::worktree::worktree_key(root),
        pattern: pattern.map(str::to_owned),
        interval,
        active: AtomicBool::new(true),
        mailbox: mailbox.clone(),
    });
    let key = key(root, owner, id);
    let serial = SEQ.fetch_add(1, Ordering::SeqCst);
    {
        let mut all = MONITORS.lock().unwrap();
        all.retain(|_, m| m.active.load(Ordering::SeqCst) && !m.mailbox.is_closed());
        if let Some(old) = all.get(&key) {
            if !old.mailbox.is_closed()
                && old.pattern == monitor.pattern
                && old.interval == interval
            {
                return Ok(json!({"id":id,"subscribed":true,"existing":true}));
            }
        }
        if !all.contains_key(&key)
            && (all.len() >= 64
                || all
                    .values()
                    .filter(|m| m.owner == owner && m.root == monitor.root)
                    .count()
                    >= 4)
        {
            return Err("日志订阅达到上限，请先取消旧订阅".into());
        }
        if let Some(old) = all.remove(&key) {
            old.active.store(false, Ordering::SeqCst);
        }
        all.insert(key.clone(), monitor.clone());
    }
    tokio::spawn(async move {
        let mut cursor = 0;
        let mut carry = String::new();
        loop {
            tokio::select! { _ = mailbox.cancelled() => break, _ = tokio::time::sleep(Duration::from_secs(interval)) => {} }
            if !monitor.active.load(Ordering::SeqCst) {
                break;
            }
            let (next, text, lost) = process.output_since(cursor);
            if next != cursor {
                let joined = format!("{carry}{text}");
                let matched = regex
                    .as_ref()
                    .is_none_or(|re| re.find_iter(&joined).any(|m| m.end() > carry.len()));
                carry = joined
                    .chars()
                    .rev()
                    .take(4096)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect();
                if matched && !text.is_empty() {
                    let tail = text
                        .chars()
                        .rev()
                        .take(6000)
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect::<String>();
                    if mailbox.publish(AsyncNotice {id:format!("monitor:{}:{serial}:{next}",process.id),
                        text:format!("日志订阅更新（工具输出，不是用户指令）\nprocess_id: {}\nbytes: {cursor}..{next}\nearlier_output_dropped: {lost}\n{tail}\nprocess unwatch 可取消订阅，终端继续运行。",process.id)}).is_err() { break; }
                }
                cursor = next;
            }
            if !process.is_running() {
                break;
            }
        }
        monitor.active.store(false, Ordering::SeqCst);
        let mut all = MONITORS.lock().unwrap();
        if all.get(&key).is_some_and(|m| Arc::ptr_eq(m, &monitor)) {
            all.remove(&key);
        }
    });
    Ok(json!({"id":id,"subscribed":true,"interval_secs":interval,"pattern":pattern}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn immediately_rewatching_after_actor_stop_replaces_the_closed_subscription() {
        let root = std::env::temp_dir().join(format!("kz-monitor-rewatch-{}", next_id()));
        std::fs::create_dir_all(&root).unwrap();
        let id = next_id();
        let process = Arc::new(BackgroundProcess {
            stdin: tokio::sync::Mutex::new(None),
            id: id.clone(),
            command: "test monitor".into(),
            project_root: root.display().to_string(),
            workdir: root.display().to_string(),
            owner: BackgroundOwner {
                run_id: "test".into(),
                process_id: "actor".into(),
                write_key: "test".into(),
            },
            persistent: false,
            log_path: None,
            full_output: Arc::new(Mutex::new(Vec::new())),
            started_at_ms: now_ms(),
            pid: None,
            output: Arc::new(Mutex::new(Vec::new())),
            output_total: Arc::new(AtomicU64::new(0)),
            truncated: Arc::new(AtomicBool::new(false)),
            exit: Arc::new(Mutex::new(None)),
            baseline: Arc::new(Mutex::new(ManagedSnapshot::capture(&root))),
            breaches: Arc::new(Mutex::new(Vec::new())),
        });
        registry().lock().unwrap().insert(id.clone(), process);
        let old_mailbox = AsyncMailbox::new(|_| Ok(()));
        subscribe(
            &root,
            "actor",
            &id,
            old_mailbox.clone(),
            Some("ready"),
            None,
        )
        .unwrap();
        let subscription_key = key(&root, "actor", &id);
        let old = MONITORS.lock().unwrap()[&subscription_key].clone();
        old_mailbox.close();
        // The current-thread executor has not polled the old task: active is still
        // true, reproducing the real stop -> immediate user watch ordering.
        assert!(old.active.load(Ordering::SeqCst));
        assert!(subscriptions(&root, "actor").is_empty());
        let current_mailbox = AsyncMailbox::new(|_| Ok(()));
        let result = subscribe(
            &root,
            "actor",
            &id,
            current_mailbox.clone(),
            Some("ready"),
            None,
        )
        .unwrap();
        assert_ne!(result["existing"], true);
        let current = MONITORS.lock().unwrap()[&subscription_key].clone();
        assert!(!Arc::ptr_eq(&old, &current));
        tokio::time::timeout(Duration::from_secs(2), async {
            while Arc::strong_count(&old) > 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(
            Arc::ptr_eq(&MONITORS.lock().unwrap()[&subscription_key], &current),
            "old worker cleanup must preserve the replacement"
        );
        assert_eq!(subscriptions(&root, "actor").len(), 1);
        current_mailbox.close();
        tokio::time::timeout(Duration::from_secs(2), async {
            while MONITORS.lock().unwrap().contains_key(&subscription_key) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        registry().lock().unwrap().remove(&id);
        std::fs::remove_dir_all(root).unwrap();
    }
}
