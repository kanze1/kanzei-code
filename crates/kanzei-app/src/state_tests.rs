//! 项目状态、配置、文档快照与缺陷审查测试。

use super::{
    export_project_data, normalized_project_root, ExportOptions, ProviderPayload, SettingsPayload,
};
use crate::docs::{docs_archive_entries, docs_snapshot};
// R-153 批10:缺陷审查迁到 subagents、模型角色校验迁到 settings。
use crate::settings::validate_model_roles;
use crate::subagents::{defect_review, defect_review_report, defect_review_snapshot};
// R-153 批5:项目隔离/分离已迁到 projects 模块,测试跟着改从模块导入。
use crate::projects::{ensure_project_isolated, project_detach};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn isolation_fixture(tag: &str, with_data: bool) -> (PathBuf, PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!(
        "kz-iso-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(base.join(".kanzei/project")).unwrap();
    if with_data {
        std::fs::write(
            base.join(".kanzei/project/requirements.md"),
            "# Requirements\n\n## R-900 上级的需求 [todo]\n",
        )
        .unwrap();
    }
    let a = base.join("projA");
    let b = base.join("projB");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    (base, a, b)
}

#[test]
fn 祖先无数据时静默自动隔离_有数据时绝不擅自改根() {
    let (base, a, _) = isolation_fixture("auto", false);
    assert!(ensure_project_isolated(&a));
    assert_eq!(
        kanzei_harness::config::discover_project_root(&a).unwrap(),
        a
    );
    assert!(!ensure_project_isolated(&a));
    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn 隔离体检一次报完全部共用项目() {
    let (base, a, b) = isolation_fixture("report", true);
    let dirs = [&a, &b];
    let shared: Vec<_> = dirs
        .iter()
        .filter(|dir| kanzei_harness::config::discover_project_root(dir).unwrap() != ***dir)
        .collect();
    assert_eq!(shared.len(), 2);
    project_detach(a.display().to_string()).unwrap();
    assert_eq!(
        kanzei_harness::config::discover_project_root(&a).unwrap(),
        a
    );
    assert_ne!(
        kanzei_harness::config::discover_project_root(&b).unwrap(),
        b
    );
    std::fs::remove_dir_all(base).ok();
}

#[test]
fn 同一上级下的两个项目必须各自独立不串数据() {
    let (base, a, b) = isolation_fixture("independent", true);
    assert_eq!(
        kanzei_harness::config::discover_project_root(&a).unwrap(),
        kanzei_harness::config::discover_project_root(&b).unwrap()
    );
    project_detach(a.display().to_string()).unwrap();
    assert_eq!(
        kanzei_harness::config::discover_project_root(&a).unwrap(),
        a
    );
    assert_ne!(
        kanzei_harness::config::discover_project_root(&a).unwrap(),
        kanzei_harness::config::discover_project_root(&b).unwrap()
    );
    assert!(!a.join(".kanzei/project/requirements.md").exists());
    std::fs::remove_dir_all(base).ok();
}

#[test]
fn 保存前拦住指向不存在_provider_的模型角色() {
    let payload = |primary: &str| SettingsPayload {
        language: None,
        primary: primary.into(),
        fast: String::new(),
        proxy: "env".into(),
        reasoning: None,
        codex_fast_mode: false,
        profile_default: None,
        profile: None,
        limits: Default::default(),
        cadence: None,
        providers: vec![ProviderPayload {
            name: "deepseek".into(),
            protocol: "openai".into(),
            base_url: "x".into(),
            api_key_env: None,
            api_key: None,
            auth: None,
            context_limit: None,
        }],
    };
    assert!(validate_model_roles(&payload("deepsek:chat")).is_err());
    assert!(validate_model_roles(&payload("deepseek:chat")).is_ok());
}

#[test]
fn defect_review_snapshot_is_strictly_read_only() {
    let root =
        std::env::temp_dir().join(format!("kanzei-defect-review-tools-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let rctx = kanzei_harness::ResolveCtx {
        profile: kanzei_harness::ProfileKind::Dev,
        cwd: root.clone(),
        project_root: root.clone(),
        config: Arc::new(kanzei_harness::KanzeiConfig::default()),
    };
    let mut names: Vec<_> = defect_review_snapshot(&rctx)
        .unwrap()
        .materialize_tools()
        .iter()
        .map(|tool| tool.name().to_string())
        .collect();
    names.sort();
    // R-218/R-234 扩容:只读快照含 files/git/glob/grep/read/symbols,全部只读。
    assert_eq!(
        names,
        vec!["files", "git", "glob", "grep", "read", "symbols"]
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn defect_review_rejects_empty_model_report() {
    let empty = kanzei_core::RunSummary {
        text: "  ".into(),
        usage: kanzei_llm::Usage::default(),
        steps: 1,
        halted_by_user: false,
        messages: vec![],
        context_report: vec![],
        overflow_traces: vec![],
    };
    assert!(defect_review_report(&empty).is_err());
}

#[tokio::test]
async fn defect_review_empty_state_returns_without_model_call() {
    let root =
        std::env::temp_dir().join(format!("kanzei-defect-review-empty-{}", std::process::id()));
    std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
    std::fs::write(root.join(".kanzei/project/defects.md"), "# Defects\n").unwrap();
    let result = defect_review(root.display().to_string()).await.unwrap();
    assert!(result.empty);
    assert_eq!(result.defect_count, 0);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn docs_snapshot_exposes_block_reasons_and_scheduler_order() {
    let root = std::env::temp_dir().join(format!("kanzei-docs-blocked-{}", std::process::id()));
    std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
    std::fs::write(
        root.join(".kanzei/project/requirements.md"),
        "# Requirements\n\n## R-001 被阻塞 [todo]\n- 阻塞: 等待确认\n\n## R-002 可执行 [todo]\n",
    )
    .unwrap();
    let requirements = docs_snapshot(root.display().to_string()).unwrap()["requirements"].clone();
    assert_eq!(requirements[0]["id"], "R-002");
    assert_eq!(requirements[1]["blocked"], true);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn docs_snapshot缓存依赖并按需加载归档正文() {
    let root = std::env::temp_dir().join(format!(
        "kanzei-docs-cache-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let project = root.join(".kanzei/project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("requirements.md"),
        "# Requirements\n\n## R-001 活跃需求 [todo]\n- 依赖: D-001\n",
    )
    .unwrap();
    std::fs::write(
        project.join("requirements-archive.md"),
        "# Requirements Archive\n\n## R-000 已归档需求 [done]\n- 进展: 已完成\n",
    )
    .unwrap();

    let started = std::time::Instant::now();
    let snapshot = docs_snapshot(root.display().to_string()).unwrap();
    let elapsed = started.elapsed();
    let ipc_bytes = serde_json::to_vec(&snapshot).unwrap().len();
    assert!(
        snapshot.get("archived_entries").is_none(),
        "归档正文不应随普通快照发送"
    );
    assert_eq!(snapshot["archived"]["req"], 1);
    let archive = docs_archive_entries(root.display().to_string(), "req".into()).unwrap();
    assert_eq!(archive[0]["id"], "R-000");
    // 同一快照数据重建旧协议的 archived_entries,得到可复核的 IPC 基线,不伪造旧实现耗时。
    let mut legacy = snapshot.clone();
    legacy["archived_entries"] = serde_json::json!({
        "req": archive,
        "defect": [],
        "goal": [],
        "source": [],
        "finding": []
    });
    let legacy_ipc_bytes = serde_json::to_vec(&legacy).unwrap().len();
    println!(
        "D-296 benchmark: IPC baseline={}bytes -> current={}bytes; current snapshot={}ms",
        legacy_ipc_bytes,
        ipc_bytes,
        elapsed.as_millis(),
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn docs_snapshot_uses_git_commits_for_live_batch_progress() {
    let root = std::env::temp_dir().join(format!(
        "kanzei-docs-git-batches-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
    std::fs::write(
        root.join(".kanzei/project/requirements.md"),
        "# Requirements\n\n## R-001 Git 真源进度 [doing]\n- 批次: 0/3\n\n## R-002 尚无提交标记 [doing]\n- 批次: 2/3\n",
    )
    .unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.email", "test@example.com"],
        vec!["config", "user.name", "Kanzei Test"],
        vec!["add", ".kanzei/project/requirements.md"],
        vec!["commit", "-q", "-m", "R-001 B2: second batch"],
        vec![
            "commit",
            "--allow-empty",
            "-q",
            "-m",
            "R-001 B1: first batch",
        ],
    ] {
        assert!(std::process::Command::new("git")
            .args(args)
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
    }
    let requirements = docs_snapshot(root.display().to_string()).unwrap()["requirements"].clone();
    let git_backed = requirements
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == "R-001")
        .unwrap();
    assert_eq!(git_backed["batches"]["done"], 2);
    assert_eq!(git_backed["batches"]["total"], 3);
    let fallback = requirements
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == "R-002")
        .unwrap();
    assert_eq!(fallback["batches"]["done"], 2);
    std::fs::remove_dir_all(root).ok();
}

fn 快照夹具(标记: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "kanzei-docs-{标记}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
    root
}

/// D-249 验收②:读失败要有可见信号,不能静默降级成空列表。
///
/// 旧实现是 `store.load().unwrap_or_default()` —— Windows 文件占用这类读失败
/// 会变成一份**「成功但空」**的快照:计数归零、列表闪空,而因为它长得像成功,
/// 下游没有任何一环会重试或报警。现在读失败向上抛,前端两处 catch 都不重绘,
/// 屏幕上留着上一份快照。
#[cfg(windows)]
#[test]
fn docs_snapshot_读失败向上报错而不是空列表() {
    use std::os::windows::fs::OpenOptionsExt;

    let root = 快照夹具("read-error");
    let 需求 = root.join(".kanzei/project/requirements.md");
    std::fs::write(&需求, "# Requirements\n\n## R-001 有条目 [todo]\n").unwrap();

    // 先确认正常路径读得到,免得下面的断言因为夹具不对而假阳性。
    let 正常 = docs_snapshot(root.display().to_string()).unwrap();
    assert_eq!(正常["requirements"].as_array().unwrap().len(), 1);

    // 独占占用 = 读不到。注意这与"文件不存在"必须区分开:后者才是"真的没有条目"。
    let 占用 = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&需求)
        .unwrap();
    let 结果 = docs_snapshot(root.display().to_string());
    drop(占用);
    let error = 结果.expect_err("读不到必须报错,不能返回空列表冒充成功");
    assert!(
        error.contains("requirements.md"),
        "错误要点名读不到的是哪个文件: {error}"
    );

    // 占用解除后自动恢复,不留任何粘滞状态。
    let 恢复 = docs_snapshot(root.display().to_string()).unwrap();
    assert_eq!(恢复["requirements"].as_array().unwrap().len(), 1);
    std::fs::remove_dir_all(root).ok();
}

/// D-249 验收①:并发写 + 读的压测下,前端不会收到「成功但空」的快照。
///
/// 这条打的是完整通道:`docs_snapshot`(自己也会写——开头的 archive_terminal)
/// 与 tracker 写并发。原来一次 refreshDocs 和一次 refreshDocsSoon 同时在飞,
/// 一个在写一个在读,读的那个就能拿到被截断的文件。
#[test]
fn docs_snapshot_并发刷新不会返回成功但空的快照() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let root = 快照夹具("concurrent");
    let 条目数 = 6usize;
    let mut text = String::from("# Requirements\n");
    for n in 1..=条目数 {
        text.push_str(&format!("\n## R-{n:03} 条目 [todo]\n- 验收: 略\n"));
    }
    std::fs::write(root.join(".kanzei/project/requirements.md"), &text).unwrap();

    let 停 = Arc::new(AtomicBool::new(false));
    // 写线程:反复整文件重写,制造与读的重叠窗口。
    let 写者 = {
        let root = root.clone();
        let 停 = Arc::clone(&停);
        std::thread::spawn(move || {
            let store = kanzei_tools::docstore::DocStore::open(
                &root,
                &kanzei_tools::docstore::REQUIREMENTS,
            );
            while !停.load(Ordering::Relaxed) {
                let entries = match store.load() {
                    Ok(entries) => entries,
                    Err(_) => continue,
                };
                store.save(&entries).unwrap();
            }
        })
    };

    let mut 轮次 = 0usize;
    for _ in 0..60 {
        let snapshot = docs_snapshot(root.display().to_string())
            .expect("并发下读失败应当罕见;真发生了也必须是错误而不是空快照");
        let 需求 = snapshot["requirements"].as_array().unwrap();
        assert_eq!(
            需求.len(),
            条目数,
            "拿到了「成功但空/缺条目」的快照:{} 条",
            需求.len()
        );
        轮次 += 1;
    }
    停.store(true, Ordering::Relaxed);
    写者.join().unwrap();
    assert!(轮次 > 0);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn export_project_data_copies_selected_work_materials() {
    let base = std::env::temp_dir().join(format!("kanzei-export-{}", std::process::id()));
    let project = base.join("project");
    let output = base.join("output");
    std::fs::create_dir_all(project.join(".kanzei/memory")).unwrap();
    std::fs::create_dir_all(project.join(".kanzei/project")).unwrap();
    std::fs::write(project.join(".kanzei/memory/M-001.md"), "记忆").unwrap();
    std::fs::write(project.join(".kanzei/project/requirements.md"), "需求").unwrap();
    std::fs::write(project.join(".kanzei/kanzei.toml"), "[models]").unwrap();
    let result = export_project_data(ExportOptions {
        project_dir: project.display().to_string(),
        output_dir: output.display().to_string(),
        include_memory: true,
        include_requirements: true,
        include_defects: false,
        include_config: true,
    })
    .unwrap();
    let export_path = PathBuf::from(result["path"].as_str().unwrap());
    assert!(export_path.join(".kanzei/memory/M-001.md").is_file());
    assert!(export_path.join(".kanzei/kanzei.toml").is_file());
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn project_root_normalizes_equivalent_paths() {
    let current = std::env::current_dir().unwrap();
    assert_eq!(
        normalized_project_root(Path::new(".")),
        std::fs::canonicalize(current).unwrap()
    );
}
