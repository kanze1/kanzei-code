#[test]
fn integrity_reports_each_unreadable_or_idless_source() {
    let dir = kanzei_base::test_support::TestDir::new("kz-memory-source-audit-");
    let store = MemoryStore::open(MemoryScope::Project, dir.to_path_buf());
    std::fs::write(
        dir.join("M-001-idless.md"),
        "---\ntitle: missing ID\n---\nbody",
    )
    .unwrap();
    std::fs::write(dir.join("M-002-unreadable.md"), [0xff, 0xfe]).unwrap();
    std::fs::create_dir(dir.join("archive")).unwrap();
    std::fs::write(dir.join("archive/M-003-broken.md"), "no frontmatter").unwrap();
    for name in ["INDEX.md", "inbox.md", "voided-ids.md"] {
        std::fs::write(dir.join(name), "metadata without an entry ID").unwrap();
    }
    let issues = store.integrity_issues();
    assert!(
        issues.iter().any(|issue| issue.contains("M-001-idless.md")),
        "{issues:?}"
    );
    assert!(
        issues
            .iter()
            .any(|issue| issue.contains("M-002-unreadable.md")),
        "{issues:?}"
    );
    assert!(
        issues
            .iter()
            .any(|issue| issue.contains("archive") && issue.contains("M-003-broken.md")),
        "{issues:?}"
    );
    assert_eq!(issues.len(), 3, "managed metadata is not a memory entry");
    assert!(
        store.load_all().is_empty(),
        "tolerant reads retain valid entries only"
    );
    assert!(
        store.load_all_checked().is_err(),
        "strict reads must reject an incomplete source"
    );
}

#[test]
fn missing_source_is_empty_but_invalid_directory_is_diagnosed() {
    let dir = kanzei_base::test_support::TestDir::new("kz-memory-directory-audit-");
    let path = dir.join("memory");
    let store = MemoryStore::open(MemoryScope::Project, path.clone());
    assert!(store.load_all_checked().unwrap().is_empty());
    assert!(store.integrity_issues().is_empty());
    std::fs::write(&path, "not a directory").unwrap();
    assert!(store.load_all_checked().is_err());
    assert!(store
        .integrity_issues()
        .iter()
        .any(|issue| issue.contains("read memory directory")));
}

#[test]
fn integrity_detects_holes_and_duplicates() {
    let (dir, store) = temp_store();
    add(&store, "fact", "一号", "钩子一", "x");
    let b = add(&store, "fact", "三号占位", "钩子三", "x");
    // 手工制造缺号:把 M-002 位空出来(改 id 为 M-003)
    store
        .update(&b.id, None, None, None, None, None, false)
        .unwrap();
    let path = store.root.join(format!("{}.md", b.file_stem()));
    let text = std::fs::read_to_string(&path)
        .unwrap()
        .replace("id: M-002", "id: M-003");
    std::fs::write(&path, text).unwrap();
    let issues = store.integrity_issues();
    assert!(issues.iter().any(|i| i.contains("M-002")), "{issues:?}");
    std::fs::remove_dir_all(dir).ok();
}

/// D-321:注销通道——缺号登记到 voided-ids.md 后 integrity 不再报 MISSING;
/// 文案在无 git 时给可执行处置(检查备份/注销),而不是误导性 git 恢复指引。
#[test]
fn void_id_acknowledges_gap_and_message_is_honest() {
    let (dir, store) = temp_store();
    add(&store, "fact", "一号", "钩子一", "x");
    let b = add(&store, "fact", "三号占位", "钩子三", "x");
    // 手工制造缺号:把 M-002 位空出(改 id 为 M-003)
    let path = store.root.join(format!("{}.md", b.file_stem()));
    let text = std::fs::read_to_string(&path)
        .unwrap()
        .replace("id: M-002", "id: M-003");
    std::fs::write(&path, text).unwrap();
    // 临时目录无 git → 文案不得指引 git 恢复,必须给可执行处置
    let issues = store.integrity_issues();
    let missing = issues
        .iter()
        .find(|i| i.contains("MISSING ids"))
        .expect("应有 MISSING 报告");
    assert!(missing.contains("no git backup"), "{missing}");
    assert!(!missing.contains("restore from git"), "{missing}");
    assert!(missing.contains("voided-ids.md"), "{missing}");
    // 注销 M-002(确认丢失)→ 不再报 MISSING
    store.void_id("M-002", "误删且无备份,确认丢失").unwrap();
    let issues = store.integrity_issues();
    assert!(
        !issues.iter().any(|i| i.contains("M-002")),
        "注销后不得再报 M-002: {issues:?}"
    );
    // 台账落盘且人可读
    let ledger = std::fs::read_to_string(store.voided_ledger_file()).unwrap();
    assert!(
        ledger.contains("- M-002: 误删且无备份,确认丢失"),
        "{ledger}"
    );
    std::fs::remove_dir_all(dir).ok();
}

/// D-321:注销前置校验——活着的条目/短理由/错前缀都拒;重复注销幂等。
#[test]
fn void_id_validates_and_is_idempotent() {
    let (dir, store) = temp_store();
    let a = add(&store, "fact", "活条目", "钩子", "x");
    let err = store.void_id(&a.id, "想清掉活条目").unwrap_err();
    assert!(err.to_string().contains("仍存在于活动或归档"), "{err}");
    let err = store.void_id("M-999", "短").unwrap_err();
    assert!(err.to_string().contains("理由"), "{err}");
    let err = store.void_id("D-999", "理由足够长但前缀错").unwrap_err();
    assert!(err.to_string().contains("不是"), "{err}");
    // 缺号注销:两次调用幂等(第二次 Ok 且不重复写行)
    store.void_id("M-999", "测试注销的缺号").unwrap();
    store.void_id("M-999", "测试注销的缺号").unwrap();
    let ledger = std::fs::read_to_string(store.voided_ledger_file()).unwrap();
    assert_eq!(ledger.matches("- M-999:").count(), 1, "{ledger}");
    std::fs::remove_dir_all(dir).ok();
}

/// D-321:登记为 voided 的编号若又出现条目(手工改号/恢复),账实不符必须可见。
#[test]
fn voided_id_resurrected_is_flagged() {
    let (dir, store) = temp_store();
    let a = add(&store, "fact", "一号", "钩子一", "x");
    let b = add(&store, "fact", "二号", "钩子二", "x");
    // 制造缺号:删掉 M-002 文件
    let path = store.root.join(format!("{}.md", b.file_stem()));
    std::fs::remove_file(&path).unwrap();
    store.void_id("M-002", "二号被误删").unwrap();
    assert!(
        !store.integrity_issues().iter().any(|i| i.contains("M-002")),
        "注销后缺号不应再报"
    );
    // 把 M-001 改号成 M-002(模拟手工恢复),复活必须被点名
    let p1 = store.root.join(format!("{}.md", a.file_stem()));
    let text = std::fs::read_to_string(&p1)
        .unwrap()
        .replace("id: M-001", "id: M-002");
    std::fs::write(&p1, text).unwrap();
    let issues = store.integrity_issues();
    assert!(
        issues
            .iter()
            .any(|i| i.contains("recorded as voided") && i.contains("M-002")),
        "{issues:?}"
    );
    std::fs::remove_dir_all(dir).ok();
}

/// D-321:文案诚实性——目录在 git 版本控制下才指引 restore from git。
#[test]
fn missing_message_honors_git_presence() {
    let (dir, store) = temp_store();
    // 临时目录根放一个 .git,祖先探测应命中
    std::fs::create_dir_all(dir.join(".git")).unwrap();
    add(&store, "fact", "一号", "钩子一", "x");
    let b = add(&store, "fact", "三号占位", "钩子三", "x");
    let path = store.root.join(format!("{}.md", b.file_stem()));
    let text = std::fs::read_to_string(&path)
        .unwrap()
        .replace("id: M-002", "id: M-003");
    std::fs::write(&path, text).unwrap();
    let issues = store.integrity_issues();
    let missing = issues
        .iter()
        .find(|i| i.contains("MISSING ids"))
        .expect("应有 MISSING 报告");
    assert!(missing.contains("restore from git"), "{missing}");
    assert!(!missing.contains("no git backup"), "{missing}");
    std::fs::remove_dir_all(dir).ok();
}
