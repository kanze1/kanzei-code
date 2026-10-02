//! 调度域独立测试(R-204 验收①):coupling_signals / dispatch_verdict / backlog_status /
//! dependents_map 的语义测试从 tracker.rs `tests` 下沉到本文件,调度逻辑可单独审计。

use super::scheduling::{backlog_status, coupling_signals, dependents_map, dispatch_verdict};
use crate::docstore::{DocStore, Entry, REQUIREMENTS};
use kanzei_harness::ToolCtx;

fn entry(id: &str) -> Entry {
    Entry {
        id: id.into(),
        title: format!("t-{id}"),
        status: "todo".into(),
        severity: None,
        fields: vec![],
    }
}

/// R-185 验收①:耦合证伪信号对真实历史条目两个方向都成立——
/// D-262/D-257/D-261(文件面不重叠)判可并行;R-177/R-182(主根重定向两半,
/// 文件面强重叠)判耦合。纯函数,构造与历史条目同构的字段即可。
#[test]
fn coupling_signals_detect_parallel_and_coupled_pairs() {
    // 可并行三缺陷:各点不同文件,refs 不互指。
    let d262 = Entry {
        id: "D-262".into(),
        title: "shell kill_tree".into(),
        status: "fixed".into(),
        severity: Some("medium".into()),
        fields: vec![(
            "复现".into(),
            "crates/kanzei-tools/src/shell.rs 的 kill_tree".into(),
        )],
    };
    let d257 = Entry {
        id: "D-257".into(),
        title: "worktrees-refresh".into(),
        status: "fixed".into(),
        severity: Some("medium".into()),
        fields: vec![(
            "复现".into(),
            "crates/kanzei-app/ui/index.html:79 按钮".into(),
        )],
    };
    let d261 = Entry {
        id: "D-261".into(),
        title: "test_record atomic".into(),
        status: "fixed".into(),
        severity: Some("medium".into()),
        fields: vec![(
            "复现".into(),
            "crates/kanzei-tools/src/test_record.rs 与 atomic_file.rs".into(),
        )],
    };
    for (a, b) in [(&d262, &d257), (&d262, &d261), (&d257, &d261)] {
        let (coupled, reasons) = coupling_signals(a, b);
        assert!(
            !coupled,
            "{} 与 {} 应判可并行,却报耦合: {reasons:?}",
            a.id, b.id
        );
    }

    // 已知耦合:R-177 与 R-182 都深度触碰 crates/kanzei-app/src/ 与
    // docs/design/deep_parallel_dev.md,且 R-182 refs 含 R-177。
    let r177 = Entry {
        id: "R-177".into(),
        title: "线绑 worktree 后端".into(),
        status: "done".into(),
        severity: None,
        fields: vec![
            ("内容".into(), "process_create 建线,ProcessHandle.worktree_path(crates/kanzei-app/src/processes.rs、src/state.rs),见 docs/design/deep_parallel_dev.md §6".into()),
            ("refs".into(), "R-050".into()),
        ],
    };
    let r182 = Entry {
        id: "R-182".into(),
        title: "撤销项目级单 writer".into(),
        status: "done".into(),
        severity: None,
        fields: vec![
            ("内容".into(), "worktree 与主根重定向,文档单份靠主根(docs/design/deep_parallel_dev.md、docs/design/parallel_read_serial_write_orchestration.md)".into()),
            ("refs".into(), "R-171 R-177 R-138".into()),
        ],
    };
    let (coupled, reasons) = coupling_signals(&r177, &r182);
    assert!(coupled, "R-177 与 R-182 应判耦合");
    assert!(
        reasons.iter().any(|r| r.contains("refs 互指")),
        "应含 refs 互指信号: {reasons:?}"
    );
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("路径重叠") && r.contains("deep_parallel_dev")),
        "应含文件面重叠信号: {reasons:?}"
    );
}

/// R-185 验收③④:判定留痕可回查,耦合时处置可执行(不是一句警告)。
#[test]
fn dispatch_verdict_records_reason_and_gives_actionable_remedy() {
    let a = Entry {
        id: "R-200".into(),
        title: "测试隔离夹具".into(),
        status: "todo".into(),
        severity: None,
        fields: vec![("内容".into(), "crates/kanzei/src/lib.rs 的 TestHome".into())],
    };
    let b = Entry {
        id: "R-201".into(),
        title: "游离行清理通道".into(),
        status: "todo".into(),
        severity: None,
        fields: vec![(
            "内容".into(),
            "crates/kanzei-tools/src/docstore.rs 的 raw_lines".into(),
        )],
    };
    // 可并行:留痕文本自包含「凭什么」
    let ok = dispatch_verdict(&a, &b);
    assert!(ok.contains("可并行"), "{ok}");
    assert!(ok.contains("耦合证伪通过"), "{ok}");
    assert!(ok.contains("refs 不相指"), "留痕要能回查判据: {ok}");

    // 耦合:处置是可执行的(串行化/合并/指定先后),不是一句警告——
    // R-201 与 R-202 都改 docstore.rs(同文件),判耦合。
    let mut c = b.clone();
    c.id = "R-202".into();
    let coupled = dispatch_verdict(&b, &c);
    assert!(coupled.contains("不得同批并行"), "{coupled}");
    assert!(coupled.contains("串行化"), "处置必须可执行: {coupled}");
    assert!(coupled.contains("合并成一条"), "处置必须可执行: {coupled}");
    assert!(coupled.contains("指定先后"), "处置必须可执行: {coupled}");
}

/// R-111 验收②:正反向依赖链接(dependents_map)——正向 id→deps、反向 id→dependents,
/// 去重排序,依赖=阻塞关系(refs 不在此列)。
#[tokio::test]
async fn dependents_map_reports_forward_and_reverse_links() {
    let dir = std::env::temp_dir().join(format!(
        "kz-dependents-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let store = DocStore::open(&dir, &REQUIREMENTS);
    let mut a = entry("R-001");
    a.fields.push(("依赖".into(), "R-002 R-003".into()));
    let b = entry("R-002");
    let mut c = entry("R-003");
    c.fields.push(("依赖".into(), "R-002".into()));
    store.save(&[a, b, c]).unwrap();
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    let loaded = DocStore::open(&dir, &REQUIREMENTS).load().unwrap();

    let (deps, dependents) = dependents_map(&ctx, &REQUIREMENTS, &loaded).unwrap();
    // 正向:R-001 → [R-002, R-003],R-003 → [R-002],R-002 无依赖。
    assert_eq!(
        deps.get("R-001").unwrap(),
        &vec!["R-002".to_string(), "R-003".to_string()]
    );
    assert_eq!(deps.get("R-003").unwrap(), &vec!["R-002".to_string()]);
    assert!(!deps.contains_key("R-002"));
    // 反向:R-002 被 R-001 与 R-003 依赖;R-003 只被 R-001 依赖;R-001 无人依赖。
    assert_eq!(
        dependents.get("R-002").unwrap(),
        &vec!["R-001".to_string(), "R-003".to_string()]
    );
    assert_eq!(dependents.get("R-003").unwrap(), &vec!["R-001".to_string()]);
    assert!(!dependents.contains_key("R-001"));

    // 去重:同一依赖写两遍只出现一次。
    let mut dup = entry("R-004");
    dup.fields.push(("依赖".into(), "R-002 R-002".into()));
    let mut a2 = entry("R-001");
    a2.fields.push(("依赖".into(), "R-002 R-003".into()));
    let mut c2 = entry("R-003");
    c2.fields.push(("依赖".into(), "R-002".into()));
    store.save(&[a2, entry("R-002"), c2, dup]).unwrap();
    let loaded = DocStore::open(&dir, &REQUIREMENTS).load().unwrap();
    let (_, dependents) = dependents_map(&ctx, &REQUIREMENTS, &loaded).unwrap();
    assert_eq!(
        dependents.get("R-002").unwrap(),
        &vec![
            "R-001".to_string(),
            "R-003".to_string(),
            "R-004".to_string()
        ]
    );
    std::fs::remove_dir_all(dir).ok();
}

/// R-169:backlog 三态(Workable/Empty/AllBlocked)——桌面端与 CLI 共用同一实现;
/// 阻塞条目不占 active,非法 lifecycle 不算活动条目。
#[test]
fn backlog_status_三态判定_桌面端与_cli共用同一实现() {
    use kanzei_harness::auto_run::BacklogStatus;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};
    let uniq = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("kz-backlog-{}-{uniq}", std::process::id()));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let root: &Path = &dir;
    // ① 全阻塞:带「阻塞:」字段的活动条目 → AllBlocked。
    let mut blocked = entry("R-001");
    blocked.status = "doing".into();
    blocked.fields = vec![("阻塞".into(), "等用户回复方案".into())];
    DocStore::open(root, &REQUIREMENTS)
        .save(&[blocked])
        .unwrap();
    DocStore::open(root, &crate::docstore::DEFECTS)
        .save(&[])
        .unwrap();
    assert!(matches!(backlog_status(root), BacklogStatus::AllBlocked));
    // ② 存在可推进条目 → Workable(即使有另一条被阻塞)。
    DocStore::open(root, &REQUIREMENTS)
        .save(&[entry("R-002")])
        .unwrap();
    assert!(matches!(backlog_status(root), BacklogStatus::Workable));
    // ③ 无活动条目 → Empty。
    DocStore::open(root, &REQUIREMENTS).save(&[]).unwrap();
    DocStore::open(root, &crate::docstore::DEFECTS)
        .save(&[])
        .unwrap();
    assert!(matches!(backlog_status(root), BacklogStatus::Empty));
    std::fs::remove_dir_all(dir).ok();
}

/// R-169 反证:读取故障必须报 Unknown,不能伪装成「已清空」(Empty)。
#[test]
fn backlog_status_读取失败时返回未知而非误报清空() {
    use kanzei_harness::auto_run::BacklogStatus;
    use std::time::{SystemTime, UNIX_EPOCH};
    let uniq = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "kz-backlog-read-fail-{}-{uniq}",
        std::process::id()
    ));
    let project = dir.join(".kanzei/project");
    std::fs::create_dir_all(project.join("defects.md")).unwrap();

    assert!(matches!(backlog_status(&dir), BacklogStatus::Unknown));
    std::fs::remove_dir_all(dir).ok();
}
