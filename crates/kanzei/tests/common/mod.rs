//! R-200:测试共用夹具——全局根(~/.kanzei)隔离。
//!
//! 凡是会读全局根的测试(子进程 spawn `kz`/`kzapp`)统一走 [`TestHome`]:
//! 建临时目录并把 `HOME` / `USERPROFILE` / `KANZEI_HOME` 三个环境变量全部
//! 指向它,drop 时清理。现状痛点(D-292):每处 spawn 手写三连,漏一个就退回
//! 读开发者本机 `~/.kanzei/kanzei.toml`——只在特定全局配置下才炸,漏很久没人发现。
//! `KANZEI_HOME` 是官方隔离通道(harness/src/home.rs:19-24 优先读它),三连缺一不可。

use std::path::PathBuf;

/// 全局根隔离 guard:建临时 HOME 目录,提供三连环境变量;drop 时整目录清理。
pub struct TestHome {
    pub root: PathBuf,
}

impl TestHome {
    /// 建临时 HOME 目录(名字带 tag 便于排障)。
    pub fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "kz-home-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    /// 全局根隔离三连:KANZEI_HOME 优先被 `kanzei_home()` 读取;HOME/USERPROFILE
    /// 一并设,防止任何直读 `dirs::home_dir()` 的路径退回开发者真实配置。
    pub fn envs(&self) -> [(&'static str, String); 3] {
        [
            ("HOME", self.root.to_string_lossy().into_owned()),
            ("USERPROFILE", self.root.to_string_lossy().into_owned()),
            (
                "KANZEI_HOME",
                self.root.join(".kanzei").to_string_lossy().into_owned(),
            ),
        ]
    }

    /// 一次性应用到 tokio 子进程命令。**用 apply 而不是三个手写 `.env(...)`**:
    /// 三连缺一不可由结构保证,杜绝「漏一个就退回读开发者真实配置」的 D-292 复发。
    pub fn apply(&self, cmd: &mut tokio::process::Command) {
        for (key, value) in self.envs() {
            cmd.env(key, value);
        }
    }
}

impl Drop for TestHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
