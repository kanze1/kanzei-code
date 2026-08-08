//! kanzei 全局数据根(≈ `~/.kanzei`)的统一解析入口。
//!
//! D-187:`KANZEI_HOME` 原本只有 memory 认,config/markdown/app 都各自拼
//! `dirs::home_dir()/.kanzei`——设了这个变量,记忆搬走了、配置与组件还留在真
//! HOME,半个覆盖比不覆盖更容易骗人。这里收敛成唯一入口:所有 `~/.kanzei`
//! 消费点(config、markdown 组件、app.json、agent-containers、memory)必须走
//! [`kanzei_home()`],禁止再各自拼路径。

use std::path::PathBuf;

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

#[cfg(test)]
mod tests {
    use super::kanzei_home;

    #[test]
    fn kanzei_home_honors_env_var() {
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
    }

    #[test]
    fn kanzei_home_defaults_to_home_dot_kanzei() {
        let Some(home) = dirs::home_dir() else {
            return; // 无 HOME 的环境跳过,不是被测行为。
        };
        assert_eq!(kanzei_home(), Some(home.join(".kanzei")));
    }
}
