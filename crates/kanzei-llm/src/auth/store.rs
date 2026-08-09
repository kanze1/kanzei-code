//! 共享凭证文件的安全写回(D-061)。
//!
//! ~/.claude/.credentials.json 与 ~/.codex/auth.json 同时被官方 CLI 读写,而对方
//! 不参与任何锁协议——加跨进程锁只能拦住 kanzei 自己的多进程,拦不住真正的并发方,
//! 还多一份卡死风险。因此这里用两条不依赖对方配合的手段:
//!
//! 1. **原子替换**:写临时文件再 rename 覆盖。truncate-then-write 中途崩溃会留下
//!    半截 JSON,下次解析直接报"请重新登录";rename 是原子的,读者要么看到旧的完整
//!    内容,要么看到新的完整内容。
//! 2. **写前重读**:刷新要走一次网络往返,这期间对方可能已经刷过并写盘了。OAuth 会
//!    轮换 refresh_token,把自己这份旧的写回去等于让双方的 refresh_token 都失效。
//!    所以落盘前重读一次,磁盘上更新就采纳对方的结果,不覆盖。

use std::path::Path;

use serde_json::Value;

use crate::error::LlmError;

/// 落盘刷新后的凭证。`disk_is_fresher(disk, mine)` 为真时放弃本次写入并返回磁盘上
/// 那份——调用方必须改用返回值构造请求头,因为自己手里的 token 可能已被对方轮换作废。
pub fn commit(
    path: &Path,
    next: &Value,
    disk_is_fresher: impl Fn(&Value, &Value) -> bool,
) -> Result<Value, LlmError> {
    if let Some(disk) = read_json(path) {
        if disk_is_fresher(&disk, next) {
            tracing::info!(
                path = %path.display(),
                "credentials on disk were refreshed concurrently; adopting them instead of overwriting"
            );
            return Ok(disk);
        }
    }
    atomic_write(path, next)?;
    Ok(next.clone())
}

fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn atomic_write(path: &Path, value: &Value) -> Result<(), LlmError> {
    let serialized = serde_json::to_string_pretty(value)
        .map_err(|e| LlmError::Config(format!("serialize {}: {e}", path.display())))?;
    // 临时文件必须与目标同目录:跨卷 rename 不是原子操作,某些实现还会直接失败。
    // 名字带 pid,避免两个 kanzei 进程互相踩临时文件。
    let tmp = path.with_extension(format!("kz{}.tmp", std::process::id()));
    std::fs::write(&tmp, serialized.as_bytes())
        .map_err(|e| LlmError::Config(format!("write {}: {e}", tmp.display())))?;

    // Windows 上目标被别的进程打开时 rename 会短暂失败,重试几次即可;
    // 彻底失败时删掉临时文件,绝不退回 truncate-then-write。
    let mut last = None;
    for attempt in 0..5 {
        match std::fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(error) => {
                last = Some(error);
                std::thread::sleep(std::time::Duration::from_millis(40 * (attempt + 1)));
            }
        }
    }
    let _ = std::fs::remove_file(&tmp);
    Err(LlmError::Config(format!(
        "原子替换 {} 失败: {}。凭证保持原样未被破坏,稍后重试或重新登录。",
        path.display(),
        last.map(|e| e.to_string()).unwrap_or_default()
    )))
}

/// 按毫秒时间戳字段比较新旧(Claude 的 claudeAiOauth.expiresAt)。
pub fn newer_by_millis(disk: &Value, mine: &Value, pointer: &str) -> bool {
    let at = |v: &Value| v.pointer(pointer).and_then(Value::as_i64);
    match (at(disk), at(mine)) {
        (Some(disk_at), Some(my_at)) => disk_at >= my_at,
        _ => false,
    }
}

/// 按 RFC3339 字段比较新旧(Codex 的 last_refresh)。
pub fn newer_by_rfc3339(disk: &Value, mine: &Value, pointer: &str) -> bool {
    let at = |v: &Value| {
        v.pointer(pointer)
            .and_then(Value::as_str)
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
    };
    match (at(disk), at(mine)) {
        (Some(disk_at), Some(my_at)) => disk_at >= my_at,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_file(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kz-cred-{}-{}-{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("auth.json")
    }

    #[test]
    fn 并发刷新不会用旧令牌覆盖对方的新令牌() {
        let path = temp_file("concurrent");
        // 官方 CLI 抢先刷新并写盘。
        std::fs::write(
            &path,
            json!({ "claudeAiOauth": { "accessToken": "theirs", "expiresAt": 3_000i64 } })
                .to_string(),
        )
        .unwrap();
        // 我们手里这份是同一轮开始前读到的,刷完只到更早的过期时间。
        let mine = json!({ "claudeAiOauth": { "accessToken": "mine", "expiresAt": 2_000i64 } });

        let adopted = commit(&path, &mine, |disk, mine| {
            newer_by_millis(disk, mine, "/claudeAiOauth/expiresAt")
        })
        .unwrap();

        assert_eq!(adopted["claudeAiOauth"]["accessToken"], "theirs");
        let on_disk: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            on_disk["claudeAiOauth"]["accessToken"], "theirs",
            "不该把对方的新令牌覆盖回旧的"
        );
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn 自己更新时正常落盘且不留临时文件() {
        let path = temp_file("write");
        std::fs::write(
            &path,
            json!({ "claudeAiOauth": { "accessToken": "old", "expiresAt": 1_000i64 } }).to_string(),
        )
        .unwrap();
        let mine = json!({ "claudeAiOauth": { "accessToken": "new", "expiresAt": 9_000i64 } });

        let written = commit(&path, &mine, |disk, mine| {
            newer_by_millis(disk, mine, "/claudeAiOauth/expiresAt")
        })
        .unwrap();

        assert_eq!(written["claudeAiOauth"]["accessToken"], "new");
        let on_disk: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(on_disk["claudeAiOauth"]["accessToken"], "new");
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "原子替换后不该留下临时文件");
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn 磁盘上是半截_json_时照常写入不被卡住() {
        // 旧版 truncate-then-write 崩溃留下的残骸:读不出来就当没有,直接以我们这份为准。
        let path = temp_file("corrupt");
        std::fs::write(&path, "{\"claudeAiOauth\": {\"accessTok").unwrap();
        let mine =
            json!({ "claudeAiOauth": { "accessToken": "recovered", "expiresAt": 5_000i64 } });

        let written = commit(&path, &mine, |disk, mine| {
            newer_by_millis(disk, mine, "/claudeAiOauth/expiresAt")
        })
        .unwrap();

        assert_eq!(written["claudeAiOauth"]["accessToken"], "recovered");
        let on_disk: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(on_disk["claudeAiOauth"]["accessToken"], "recovered");
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn codex_按_last_refresh_判定新旧() {
        let disk = json!({ "last_refresh": "2026-08-08T10:00:00Z" });
        let mine = json!({ "last_refresh": "2026-08-08T09:00:00Z" });
        assert!(newer_by_rfc3339(&disk, &mine, "/last_refresh"));
        assert!(!newer_by_rfc3339(&mine, &disk, "/last_refresh"));
        // 缺字段时不认为磁盘更新,以本次刷新为准。
        assert!(!newer_by_rfc3339(&json!({}), &mine, "/last_refresh"));
    }
}
