#[test]
fn add_and_note_carry_refs_contract() {
    // R-070:add 带 refs 写入 frontmatter,读取方 refs() 还原;草稿带 refs 贯通到候选列表。
    let (dir, store) = temp_store();
    let entry = match store
        .add(
            "fact",
            "CRLF 是 edit 未命中主因",
            "换行符问题必读",
            "正文",
            "user",
            &["R-070".into(), "D-200".into()],
            None,
            false,
        )
        .unwrap()
    {
        AddOutcome::Added(e) => e,
        AddOutcome::Duplicate(e) => panic!("unexpected duplicate {}", e.id),
        AddOutcome::SubjectConflict(e) => panic!("unexpected subject conflict with {}", e.id),
        AddOutcome::Uncertain(cands) => panic!("unexpected uncertain: {:?}", cands),
    };
    assert_eq!(entry.refs(), vec!["R-070".to_string(), "D-200".to_string()]);
    // 落盘文件里真能看到 refs 键,重读后仍还原。
    let (path, _) = store
        .load_all()
        .into_iter()
        .find(|(_, e)| e.id == entry.id)
        .unwrap();
    let file_text = std::fs::read_to_string(&path).unwrap();
    assert!(file_text.contains("refs: R-070 D-200"), "{file_text}");
    let reloaded = store
        .load_all()
        .into_iter()
        .find(|(_, e)| e.id == entry.id)
        .unwrap()
        .1;
    assert_eq!(reloaded.refs(), entry.refs());
    // 无 refs 时不写键。
    let plain = add(&store, "fact", "无来源条目", "普通", "x");
    assert!(plain.refs().is_empty());
    // 草稿贯通:refs 行进入 inbox,候选列表 detail 可见。
    store
        .append_note("踩坑", "细节说明", "fact", &["R-070".into()])
        .unwrap();
    let (_, summary, detail) = store.pending_note_list().pop().unwrap();
    assert_eq!(summary, "踩坑");
    assert!(detail.contains("refs: R-070"), "{detail}");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn legacy_memory_md_migrates_once() {
    let dir = std::env::temp_dir().join(format!(
        "kz-memory-migrate-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    std::fs::write(
        dir.join(".kanzei/project/memory.md"),
        "# Memory\n\n## M-001 子代理网络错误会整体失败 [active]\n- 依据: 实测\n\n## M-002 已推翻的结论 [stale]\n- 备注: 旧\n",
    )
    .unwrap();
    let store = MemoryStore::project(&dir);
    let entries = store.load_all();
    // R-165 批3:迁移的 stale 条目(deprecated)自动归档,主目录只留 active。
    assert_eq!(
        entries.len(),
        1,
        "deprecated 迁移条目应已归档: {:?}",
        entries
    );
    let m1 = entries.iter().find(|(_, e)| e.id == "M-001").unwrap();
    assert_eq!(m1.1.category, "fact");
    assert_eq!(m1.1.source, "migration");
    assert!(m1.1.body.contains("依据: 实测"));
    let archived_m2 = std::fs::read_dir(store.archive_dir())
        .unwrap()
        .flatten()
        .find(|p| {
            p.path()
                .file_stem()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("M-002"))
                .unwrap_or(false)
        })
        .expect("M-002 归档墓碑应存在");
    let m2_text = std::fs::read_to_string(archived_m2.path()).unwrap();
    assert!(m2_text.contains("status: deprecated"), "{m2_text}");
    // 原文件变为指路牌,重复 open 不再迁移
    let legacy = std::fs::read_to_string(dir.join(".kanzei/project/memory.md")).unwrap();
    assert!(legacy.contains("已迁移"));
    let again = MemoryStore::project(&dir);
    assert_eq!(again.load_all().len(), 1);
    std::fs::remove_dir_all(dir).ok();
}
