//! R-200 验收②:测试代码出现 `.env("USERPROFILE",`(或 JS 的 `USERPROFILE:`)而同一
//! 文件没有 `KANZEI_HOME` 即判红——隔离三连缺一,子进程就退回读开发者真实
//! `~/.kanzei/kanzei.toml`(D-292:漏第三个变量,只在特定全局配置下才炸,漏很久没人发现)。
//! 正确形态:统一走 `tests/common/mod.rs` 的 `TestHome::apply()`(三连由结构保证)。

use std::path::PathBuf;

/// 扫描会 spawn 子进程的测试与脚本:设置 USERPROFILE 隔离全局根的同时必须设
/// KANZEI_HOME(官方隔离通道,harness/src/home.rs 优先读它)。
#[test]
fn test_spawns_isolate_kanzei_home_alongside_userprofile() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let mut files: Vec<PathBuf> = std::fs::read_dir(repo_root.join("crates/kanzei/tests"))
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "rs"))
        .map(|e| e.path())
        .collect();
    files.push(repo_root.join("scripts/e2e-smoke.mjs"));
    files.push(repo_root.join("scripts/probe-webview-cdp.mjs"));
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        let sets_userprofile =
            text.contains(".env(\"USERPROFILE\",") || text.contains("USERPROFILE:");
        if sets_userprofile {
            assert!(
                text.contains("KANZEI_HOME"),
                "{}: 设置了 USERPROFILE 隔离全局根却没同时设 KANZEI_HOME——子进程会退回\
                 读开发者真实 ~/.kanzei(D-292 形态)。统一走 tests/common/mod.rs 的 TestHome。",
                file.display()
            );
        }
    }
}
