//! kanzei 全局数据根(≈ `~/.kanzei`)的统一解析入口。
//!
//! D-187:`KANZEI_HOME` 原本只有 memory 认,config/markdown/app 都各自拼
//! `dirs::home_dir()/.kanzei`——设了这个变量,记忆搬走了、配置与组件还留在真
//! HOME,半个覆盖比不覆盖更容易骗人。这里收敛成唯一入口:所有 `~/.kanzei`
//! 消费点(config、markdown 组件、app.json、agent-containers、memory)必须走
//! [`kanzei_home()`],禁止再各自拼路径。

use std::path::{Path, PathBuf};

/// kanzei 全局数据根目录。
///
/// 优先级:
/// 1. `KANZEI_HOME` 环境变量——测试与多实例隔离的官方通道(memory 原有语义,
///    D-187 提升为全局);
/// 2. `dirs::home_dir()/.kanzei`(Windows 上为 `%USERPROFILE%\.kanzei`)。
///
/// 返回 `None` 仅当环境连 home 目录都解析不出来(此时各消费点按各自约定回退)。
pub fn kanzei_home() -> Option<PathBuf> {
    resolve_home(std::env::var_os("KANZEI_HOME"), dirs::home_dir())
}

fn resolve_home(
    override_home: Option<std::ffi::OsString>,
    user_home: Option<PathBuf>,
) -> Option<PathBuf> {
    override_home
        .map(PathBuf::from)
        .or_else(|| user_home.map(|h| h.join(".kanzei")))
}

/// The projectless store is an application-owned scope, never a discovered project.
pub fn is_general_conversation_root(root: &Path) -> bool {
    kanzei_home().is_some_and(|home| is_general_conversation_root_at(root, &home))
}

fn is_general_conversation_root_at(root: &Path, home: &Path) -> bool {
    crate::project_root::is_same_dir(root, &home.join("conversations/general"))
}

/// Files belong to a conversation; shared history and memory stay in the store root.
pub fn general_conversation_workspace(root: &Path, session: &str) -> PathBuf {
    root.join("artifacts")
        .join(kanzei_base::content_hash(session.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::{is_general_conversation_root_at, resolve_home};

    #[test]
    fn canonical_general_root_keeps_identity_with_dot_home() {
        let base = std::env::temp_dir().join(format!(
            "kanzei-general-home-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(base.join("padding")).unwrap();
        let home = base.join("padding").join("..").join("data");
        let general = home.join("conversations/general");
        std::fs::create_dir_all(&general).unwrap();
        let canonical = std::fs::canonicalize(&general).unwrap();
        assert!(is_general_conversation_root_at(&canonical, &home));
        assert!(!is_general_conversation_root_at(
            &canonical.join("artifacts"),
            &home
        ));
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn home_override_default_and_absence_are_independent_inputs() {
        let home = std::path::PathBuf::from("user-home");
        let override_home = std::ffi::OsString::from("isolated-home");
        assert_eq!(
            resolve_home(Some(override_home), Some(home.clone())),
            Some("isolated-home".into())
        );
        assert_eq!(
            resolve_home(None, Some(home.clone())),
            Some(home.join(".kanzei"))
        );
        assert_eq!(resolve_home(None, None), None);
    }
}
