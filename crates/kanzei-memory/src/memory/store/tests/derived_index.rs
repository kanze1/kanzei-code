#[test]
fn index_description_guard_rejects_mismatched_source() {
    let (dir, store) = temp_store();
    let entry = add(
        &store,
        "fact",
        "INDEX guard — title",
        "INDEX guard — description",
        "INDEX guard body",
    );
    let entries = store.load_all();
    let valid = format!(
        "- {} [{}] {} — {}\n",
        entry.id, entry.category, entry.title, entry.description
    );
    assert!(MemoryStore::assert_index_matches_entries(&valid, &entries).is_ok());
    let invalid = valid.replace("INDEX guard — description", "wrong description");
    assert!(MemoryStore::assert_index_matches_entries(&invalid, &entries).is_err());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn repair_derived_rebuilds_mismatched_index_from_markdown_source() {
    let (dir, store) = temp_store();
    let entry = add(
        &store,
        "fact",
        "repair index title",
        "repair index description",
        "repair index body",
    );
    std::fs::write(
        store.index_md(),
        "# Memory Index (project)\n\n- M-001 [fact] wrong — wrong\n",
    )
    .unwrap();
    store.repair_derived().unwrap();
    let repaired = std::fs::read_to_string(store.index_md()).unwrap();
    assert!(repaired.contains(&format!(
        "- {} [fact] {} — {}",
        entry.id, entry.title, entry.description
    )));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn status_filter_is_applied_before_fts_limit() {
    let (dir, store) = temp_store();
    for i in 0..30 {
        let outcome = store
            .add(
                "fact",
                &format!("状态窗口候选 {i}"),
                "状态窗口检索",
                "状态窗口正文",
                "memory-manager",
                &[],
                None,
                false,
            )
            .unwrap();
        assert!(matches!(outcome, AddOutcome::Added(ref entry) if entry.status == "candidate"));
    }
    let active = add(
        &store,
        "fact",
        "状态窗口 active",
        "状态窗口检索",
        "状态窗口正文",
    );
    let rows = store
        .search_candidates("状态窗口", None, Some("active"))
        .unwrap();
    assert_eq!(
        rows.len(),
        1,
        "active 查询不能被 candidate 挤出 LIMIT 窗口: {rows:?}"
    );
    assert_eq!(rows[0].entry.id, active.id);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn 检索守护_外部写入的文件失步后自动重建索引() {
    // 2026-08-13 清理事故形态:手动移动/写入 .md 绕过写路径的增量维护,
    // FTS 失步——新条目对 BM25 完全不可见、已归档条目仍在索引。
    // search 前的 id 集合守护必须自动 refresh_derived。
    let (dir, store) = temp_store();
    add(&store, "fact", "常规条目", "常规检索钩子", "正文 A");
    // 外部写入:直接落文件,不走 add(不触发派生物维护)。
    std::fs::write(
        store.root.join("M-099-外部恢复条目.md"),
        "---\nid: M-099\nscope: project\ncategory: sop\ntitle: 外部恢复条目 quasar 约束\ndescription: 外部写入的检索钩子 quasar\nstatus: active\ncreated: 2026-08-13\nupdated: 2026-08-13\nsource: test\n---\n\n正文 quasar",
    )
    .unwrap();
    let rows = store
        .search_candidates("quasar", None, Some("active"))
        .unwrap();
    assert!(
        rows.iter().any(|r| r.entry.id == "M-099"),
        "失步守护必须重建索引,外部写入的条目才可检索: {rows:?}"
    );
    // 反向:外部删除(归档)后,索引里的幽灵条目不再命中。
    std::fs::remove_file(store.root.join("M-099-外部恢复条目.md")).unwrap();
    let rows = store
        .search_candidates("quasar", None, Some("active"))
        .unwrap();
    assert!(
        rows.iter().all(|r| r.entry.id != "M-099"),
        "外部删除后幽灵条目不得再命中: {rows:?}"
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn 写入前发现失步会自动重建_fts并与主目录对齐() {
    let (dir, store) = temp_store();
    let first = add(&store, "fact", "写入前守护首条", "失步守护检索", "正文");
    {
        let conn = store.open_db().unwrap();
        conn.execute("DELETE FROM memory_fts WHERE id = ?1", params![first.id])
            .unwrap();
    }

    let second = add(&store, "fact", "写入前守护次条", "失步守护检索", "正文二");
    let conn = store.open_db().unwrap();
    let indexed: i64 = conn
        .query_row("SELECT COUNT(*) FROM memory_fts", [], |row| row.get(0))
        .unwrap();
    assert_eq!(indexed as usize, store.load_all().len());
    for id in [first.id, second.id] {
        let present: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM memory_fts WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(present, 1, "写入后 FTS 缺少 {id}");
    }
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn add_assigns_ids_and_builds_derived_index() {
    let (dir, store) = temp_store();
    let a = add(
        &store,
        "fact",
        "CRLF 是 edit 未命中主因",
        "换行符问题必读",
        "正文 A",
    );
    let b = add(
        &store,
        "sop",
        "发版 SOP",
        "做发版相关任务必读",
        "1. 测试 2. 推送 3. 发布",
    );
    assert_eq!((a.id.as_str(), b.id.as_str()), ("M-001", "M-002"));
    let index = std::fs::read_to_string(store.root.join("INDEX.md")).unwrap();
    assert!(index.contains("M-001 [fact] CRLF 是 edit 未命中主因 — 换行符问题必读"));
    assert!(index.contains("M-002 [sop]"));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn candidate_not_in_default_fts_or_index_and_index_descriptions_match() {
    let (dir, store) = temp_store();
    let active = match store
        .add(
            "fact",
            "active description source",
            "active source description",
            "active body",
            "user",
            &[],
            None,
            false,
        )
        .unwrap()
    {
        AddOutcome::Added(entry) => entry,
        other => panic!("active fixture 应写入: {other:?}"),
    };
    let candidate = match store
        .add(
            "fact",
            "candidate hidden from production search",
            "candidate source description",
            "候选不可检索单轨标记",
            "memory-manager",
            &[],
            None,
            true,
        )
        .unwrap()
    {
        AddOutcome::Added(entry) => entry,
        other => panic!("candidate fixture 应写入: {other:?}"),
    };
    assert_eq!(candidate.status, "candidate");

    let index_text = std::fs::read_to_string(store.index_md()).unwrap();
    assert!(index_text.contains(&format!(
        "- {} [fact] {} — {}",
        active.id, active.title, active.description
    )));
    assert!(!index_text
        .lines()
        .any(|line| line.starts_with(&format!("- {} [", candidate.id))));
    assert!(index_text.contains("(1 candidate 条待验证晋升)"));

    let fts_hits = store
        .search_candidates("候选 不可 检索 单轨 标记", None, Some("active"))
        .unwrap();
    assert!(
        fts_hits.iter().all(|hit| hit.entry.id != candidate.id),
        "candidate 不得进入默认 FTS 检索: {:?}",
        fts_hits.iter().map(|hit| &hit.entry.id).collect::<Vec<_>>()
    );
    let index = SqliteMemoryIndex::new(&dir);
    let production_hits = index
        .search_entries(
            &IndexQuery::text("候选 不可 检索 单轨 标记"),
            None,
            Some("active"),
            5,
        )
        .unwrap();
    assert!(
        production_hits
            .iter()
            .all(|hit| hit.entry.id != candidate.id),
        "candidate 不得进入主检索: {:?}",
        production_hits
            .iter()
            .map(|hit| &hit.entry.id)
            .collect::<Vec<_>>()
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn same_id_external_edits_refresh_fts_and_failed_publish_rolls_back() {
    let (_dir, store) = temp_store();
    let mut entry = add(
        &store,
        "fact",
        "source revision",
        "source revision",
        "oldrevisiontoken",
    );
    let path = store
        .load_all()
        .into_iter()
        .find(|(_, e)| e.id == entry.id)
        .unwrap()
        .0;
    entry.body = "newrevisiontoken".into();
    std::fs::write(&path, render_entry(&entry)).unwrap();
    let hits = store
        .search_candidates("newrevisiontoken", None, Some("active"))
        .unwrap();
    assert!(hits.iter().any(|hit| hit.entry.id == entry.id));
    assert!(store
        .search_candidates("oldrevisiontoken", None, Some("active"))
        .unwrap()
        .is_empty());
    let conn = store.open_db().unwrap();
    conn.execute_batch("CREATE TRIGGER reject_revision BEFORE INSERT ON memory_fts_revisions BEGIN SELECT RAISE(ABORT, 'injected publication failure'); END;").unwrap();
    entry.body = "failedrevisiontoken".into();
    std::fs::write(path, render_entry(&entry)).unwrap();
    assert!(store.ensure_derived_consistent().is_err());
    let body: String = conn
        .query_row(
            "SELECT body FROM memory_fts WHERE id=?1",
            [&entry.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        body, "newrevisiontoken",
        "FTS delete and revision publish must roll back as one transaction"
    );
}
