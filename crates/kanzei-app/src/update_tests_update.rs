//! 更新、安装校验与模型服务测试。

use super::fast_model::{ollama_service_up, pull_progress_text};

#[test]
fn 拉取进度行解析成人话且无进度字段时只给状态() {
    let with_progress = serde_json::json!({
        "status": "pulling 9f3c…", "completed": 1_572_864_000u64, "total": 3_145_728_000u64
    });
    let text = pull_progress_text(&with_progress).unwrap();
    assert!(text.contains("50%"), "{text}");
    assert!(
        text.contains("1500/3000 MB"),
        "要给出已下/总量,证明还活着: {text}"
    );

    let plain = serde_json::json!({ "status": "verifying sha256 digest" });
    assert_eq!(
        pull_progress_text(&plain).unwrap(),
        "verifying sha256 digest"
    );
    assert!(pull_progress_text(&serde_json::json!({})).is_none());
}

#[tokio::test]
async fn 服务探测对未监听端口干脆返回_false_不悬挂() {
    let started = std::time::Instant::now();
    assert!(!ollama_service_up("http://127.0.0.1:1/v1").await);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "探测未监听端口不该等这么久: {:?}",
        started.elapsed(),
    );
}

#[test]
fn installer_validation_rejects_truncated_and_non_executable_payloads() {
    let html = format!("<html>{}</html>", "x".repeat(2 << 20));
    let error = super::validate_installer(html.as_bytes()).unwrap_err();
    assert!(error.contains("不是 Windows 可执行文件"), "{error}");
    let mut short = b"MZ".to_vec();
    short.extend(std::iter::repeat_n(0u8, 4096));
    let error = super::validate_installer(&short).unwrap_err();
    assert!(error.contains("不完整"), "{error}");
    let mut good = b"MZ".to_vec();
    good.extend(std::iter::repeat_n(0u8, 2 << 20));
    assert!(super::validate_installer(&good).is_ok());
}

#[test]
fn install_helper_waits_for_the_caller_to_exit_before_installing() {
    let started = std::time::Instant::now();
    let exited =
        super::wait_for_parent_exit(std::process::id(), std::time::Duration::from_millis(600));
    let waited = started.elapsed();
    assert!(!exited, "当前进程显然活着,不该判定为已退出");
    assert!(waited >= std::time::Duration::from_millis(600));
    let started = std::time::Instant::now();
    let exited = super::wait_for_parent_exit(0xFFFF_FFF0, std::time::Duration::from_secs(30));
    assert!(exited, "父进程已退出时应立即放行");
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}

#[test]
fn release_check_never_downgrades_a_newer_local_build() {
    assert!(!super::release_is_newer(
        "local 20260809120000",
        "build-remote",
        Some("2026-08-08T23:00:00Z")
    ));
    assert!(super::release_is_newer(
        "local 20260808120000",
        "build-remote",
        Some("2026-08-09T00:00:00Z")
    ));
    assert!(!super::release_is_newer(
        "local 20260808120000",
        "build-local",
        Some("2026-08-10T00:00:00Z")
    ));
}

#[test]
fn legacy_date_only_build_requires_a_later_release_day() {
    assert!(!super::release_is_newer(
        "local 2026-08-08",
        "build-remote",
        Some("2026-08-08T23:00:00Z")
    ));
    assert!(super::release_is_newer(
        "local 2026-08-08",
        "build-remote",
        Some("2026-08-09T00:00:00Z")
    ));
    assert!(!super::release_is_newer(
        "local",
        "build-remote",
        Some("2026-08-09T00:00:00Z")
    ));
}

#[test]
fn cli同步只升不降且识别不出版本时按旧处理() {
    let ours = "0c9f903 20260808120442";
    assert!(super::installed_cli_is_older(
        "kanzei 0.1.0 (430d6d6 20260808015943)\n",
        ours
    ));
    assert!(!super::installed_cli_is_older(
        "kanzei 0.1.0 (abcdef1 20260809090000)\n",
        ours
    ));
    assert!(!super::installed_cli_is_older(
        "kanzei 0.1.0 (0c9f903 20260808120442)\n",
        ours
    ));
    for unknown in ["", "kanzei 0.1.0\n", "garbage", "kanzei 0.1.0 (dev)\n"] {
        assert!(super::installed_cli_is_older(unknown, ours), "{unknown:?}");
    }
}

#[test]
fn pending_path_uses_executable_sibling() {
    assert_eq!(
        super::pending_path(std::path::Path::new(r"C:\\bin\\kzapp.exe")),
        std::path::Path::new(r"C:\\bin\\kzapp.exe.pending")
    );
}

#[test]
fn 更新交接helper跑在安装目录之外() {
    let helper = super::update_helper_path();
    let temp = std::env::temp_dir();
    assert!(helper.starts_with(&temp));
    assert_ne!(
        helper.file_name().and_then(|n| n.to_str()),
        Some("kzapp.exe")
    );
    let test_log = temp.join(format!(
        "kanzei-update-test-{}-{}.log",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let probe_marker = format!("单测探针-{}", std::process::id());
    super::update_log_at(&test_log, &probe_marker);
    assert!(std::fs::read_to_string(&test_log)
        .unwrap()
        .contains(&probe_marker));
    let _ = std::fs::remove_file(&test_log);
}
