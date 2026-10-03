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
    std::env::var("KANZEI_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".kanzei")))
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
    use super::{is_general_conversation_root_at, kanzei_home};

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

    /// 两个 home 行为合并成顺序测试:`kanzei_home_honors_env_var` 用进程级全局
    /// KANZEI_HOME,与 `kanzei_home_defaults_to_home_dot_kanzei` 并行跑会互踩
    /// (set_var 期间默认分支读到被污染的变量,全量偶发红)。顺序执行后互斥。
    #[test]
    fn kanzei_home_顺序验证环境变量与默认() {
        // 1) 环境变量优先。
        let root = std::env::temp_dir().join(format!(
            "kanzei-home-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::env::set_var("KANZEI_HOME", &root);
        assert_eq!(kanzei_home(), Some(root.clone()));
        std::env::remove_var("KANZEI_HOME");
        assert_ne!(kanzei_home(), Some(root));

        // 2) 无变量时回落 HOME/.kanzei。
        let Some(home) = dirs::home_dir() else {
            return; // 无 HOME 的环境跳过,不是被测行为。
        };
        assert_eq!(kanzei_home(), Some(home.join(".kanzei")));
    }
}
