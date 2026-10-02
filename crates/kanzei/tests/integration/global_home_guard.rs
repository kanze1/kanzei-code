//! R-200 验收②:测试代码出现 `.env("USERPROFILE",`(或 JS 的 `USERPROFILE:`)而同一
//! 文件没有 `KANZEI_HOME` 即判红——隔离三连缺一,子进程就退回读开发者真实
//! `~/.kanzei/kanzei.toml`(D-292:漏第三个变量,只在特定全局配置下才炸,漏很久没人发现)。
//! 正确形态:统一走 `tests/common/mod.rs` 的 `TestHome::apply()`(三连由结构保证)。

use std::path::PathBuf;

/// 递归收集 `tests/` 下的 `.rs`。这里必须递归:集成测试合并成单一 target 后源文件
/// 都在 `tests/integration/` 子目录里,原先的非递归 read_dir 会一个文件都扫不到,
/// 于是本守护静默全绿、形同虚设。下面的下限断言就是防这一手。
fn collect_rs(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// 扫描会 spawn 子进程的测试:设置 USERPROFILE 隔离全局根的同时必须设
/// KANZEI_HOME(官方隔离通道,harness/src/home.rs 优先读它)。
#[test]
fn test_spawns_isolate_kanzei_home_alongside_userprofile() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let mut files: Vec<PathBuf> = Vec::new();
    collect_rs(&repo_root.join("crates/kanzei/tests"), &mut files);
    assert!(
        files.len() >= 10,
        "tests/ 下只扫到 {} 个 .rs——目录结构变了而本守护没跟着改。\
         扫不到文件时断言会全部跳过、测试照样绿,那时它已经不再保护任何东西。",
        files.len()
    );
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
