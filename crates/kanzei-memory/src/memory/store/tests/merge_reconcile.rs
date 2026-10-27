#[test]
fn merge_keeps_primary_and_tombstones_duplicates() {
    let (dir, store) = temp_store();
    let a = add(
        &store,
        "habit",
        "gh 走代理",
        "gh 网络问题必读",
        "端口 12000",
    );
    let b = add(
        &store,
        "habit",
        "gh 需要 HTTPS_PROXY",
        "gh 超时必读",
        "同上",
    );
    let merged = store
        .merge(
            &a.id,
            std::slice::from_ref(&b.id),
            None,
            Some("gh 网络失败/超时必读"),
            Some("HTTPS_PROXY=http://127.0.0.1:12000"),
            true, // 测试无共享指纹:confirmed=true 模拟用户确认(R-165 保守闸)
        )
        .unwrap();
    assert_eq!(merged.id, a.id);
    assert_eq!(merged.description, "gh 网络失败/超时必读");
    // R-165 批3:墓碑条目归档到 archive/(主目录只留 active/candidate)。
    assert!(store.load_all().iter().all(|(_, e)| e.id != b.id));
    let dup_path = std::fs::read_dir(store.archive_dir())
        .unwrap()
        .flatten()
        .find(|p| {
            p.path()
                .file_stem()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with(&b.id))
                .unwrap_or(false)
        })
        .expect("archive 墓碑应存在")
        .path();
    let dup_text = std::fs::read_to_string(dup_path).unwrap();
    assert!(dup_text.contains("status: deprecated"));
    assert!(
        dup_text.contains(&format!("superseded_by: {}", a.id)),
        "{dup_text}"
    );
    // 未知 id 与自我合并都拒绝
    assert!(store
        .merge(&a.id, std::slice::from_ref(&a.id), None, None, None, true)
        .is_err());
    assert!(store
        .merge("M-999", std::slice::from_ref(&b.id), None, None, None, true)
        .is_err());
    std::fs::remove_dir_all(dir).ok();
}

/// R-165 批4 merge 保守闸(⑧):评估器落地前只合并同 fingerprint 或用户确认的。
#[test]
fn reconcile_deduplicates_fingerprint_and_normalized_title_clusters() {
    let (dir, store) = temp_store();
    let fingerprint_primary = add(
        &store,
        "fact",
        "fingerprint primary",
        "primary description",
        "最完整正文 primary",
    );
    let fingerprint_duplicate = match store
        .add(
            "fact",
            "fingerprint duplicate",
            "duplicate description",
            "较短正文 duplicate",
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
    for entry in [&fingerprint_primary, &fingerprint_duplicate] {
        let path = store.root.join(format!("{}.md", entry.file_stem()));
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str("\n\n[fp:dedup|same failure]");
        std::fs::write(path, text).unwrap();
    }

    let title_primary = add(
        &store,
        "habit",
        "Same Merge Title!",
        "title primary",
        "title primary body",
    );
    let title_duplicate = add(
        &store,
        "habit",
        "temporary unique title",
        "title duplicate",
        "title duplicate body",
    );
    let title_duplicate_path = store
        .root
        .join(format!("{}.md", title_duplicate.file_stem()));
    let title_duplicate_text = std::fs::read_to_string(&title_duplicate_path)
        .unwrap()
        .replace("title: temporary unique title", "title: same merge title")
        .replace("status: active", "status: candidate");
    std::fs::write(title_duplicate_path, title_duplicate_text).unwrap();
    let unrelated = add(
        &store,
        "habit",
        "unrelated title",
        "unrelated description",
        "unrelated body",
    );
    let unrelated_two = add(
        &store,
        "habit",
        "another unrelated title",
        "another description",
        "another body",
    );

    let report = store.reconcile_candidates(None, 365).unwrap();
    assert!(report.merged.contains(&fingerprint_duplicate.id));
    assert!(report.merged.contains(&title_duplicate.id));
    assert!(!report.merged.contains(&unrelated.id));
    assert!(!report.merged.contains(&unrelated_two.id));
    assert!(store.load_all().iter().all(|(_, entry)| {
        entry.id != fingerprint_duplicate.id && entry.id != title_duplicate.id
    }));
    assert!(store.load_all().iter().any(|(_, entry)| {
        entry.id == fingerprint_primary.id && entry.body.contains("[fp:dedup|same failure]")
    }));
    assert!(store
        .load_all()
        .iter()
        .any(|(_, entry)| entry.id == title_primary.id));
    let archived_duplicate = std::fs::read_dir(store.archive_dir())
        .unwrap()
        .flatten()
        .find(|path| {
            path.path()
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .starts_with(&fingerprint_duplicate.id)
        })
        .expect("fingerprint duplicate 应进入 archive");
    let archived_text = std::fs::read_to_string(archived_duplicate.path()).unwrap();
    assert!(archived_text.contains(&format!("superseded_by: {}", fingerprint_primary.id)));
    let index = std::fs::read_to_string(store.index_md()).unwrap();
    assert!(index.contains(&fingerprint_primary.id));
    assert!(!index.contains(&fingerprint_duplicate.id));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn merge_conservative_gate_requires_shared_fingerprint_or_confirmed() {
    let (dir, store) = temp_store();
    // R-216:指纹必须来自来源 note 才放行——先注入来源,再测 merge 保守闸
    // (保守闸本身验证共享指纹放行,不是指纹闸)。
    store
        .append_note("edit 未命中来源", "[fp:edit|not found]", "fact", &[])
        .unwrap();
    let a = add(&store, "fact", "主题甲", "钩子甲", "正文甲");
    let b = add(&store, "fact", "主题乙", "钩子乙", "正文乙");
    // 无 confirmed、无共享指纹 → 拒绝。
    let err = store
        .merge(&a.id, std::slice::from_ref(&b.id), None, None, None, false)
        .unwrap_err();
    assert!(
        err.to_string().contains("无共享 fingerprint"),
        "保守闸应点名缺共享指纹: {err}"
    );
    // 有共享指纹 → 放行。
    let c = store
        .add(
            "fact",
            "同坑另一个角度",
            "钩子丙",
            "正文 [fp:edit|not found]",
            "user",
            &[],
            None,
            false,
        )
        .unwrap();
    let AddOutcome::Added(c) = c else {
        panic!("expected Added")
    };
    let d = store
        .add(
            "fact",
            "同坑第三个角度",
            "钩子丁",
            "补充内容",
            "user",
            &[],
            None,
            false,
        )
        .unwrap();
    let AddOutcome::Added(d) = d else {
        panic!("expected Added")
    };
    // 既有 legacy 条目可能在去重门禁上线前已带同一指纹；用 update 构造该存量
    // 场景，再验证 merge 的共享指纹保守闸，而不是绕过新的 add 门禁。
    store
        .update(
            &d.id,
            None,
            None,
            Some("补充 [fp:edit|not found]"),
            None,
            None,
            false,
        )
        .unwrap();
    let merged = store
        .merge(&c.id, std::slice::from_ref(&d.id), None, None, None, false)
        .unwrap();
    assert_eq!(merged.id, c.id);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn merge_自动搬运被并条目的指纹与refs() {
    // D-215:合并不许静默丢掉复发检测键与来源链——这不能赌 manager 记得带。
    let (dir, store) = temp_store();
    // R-216:指纹来源——先注入 inbox 来源 note,再 add 携带同指纹(测 merge 兜底)。
    store
        .append_note("edit 未命中来源", "[fp:edit|not found]", "fact", &[])
        .unwrap();
    let a = add(&store, "fact", "edit 未命中主因", "edit 失败必读", "判据 A");
    let b = match store
        .add(
            "fact",
            "edit 未命中另一坑",
            "edit 失败必读 2",
            "判据 B [fp:edit|not found]",
            "user",
            &["R-001".into()],
            None,
            false,
        )
        .unwrap()
    {
        AddOutcome::Added(e) => e,
        _ => panic!("expected add"),
    };
    store.set_area(&a.id, &["app/frontend".into()]).unwrap();
    store.set_area(&b.id, &["engine/ipc".into()]).unwrap();
    let merged = store
        .merge(
            &a.id,
            std::slice::from_ref(&b.id),
            None,
            None,
            Some("合并后的正文(manager 忘了带指纹)"),
            true, // 测指纹兜底而非闸门:confirmed=true 放行
        )
        .unwrap();
    // 指纹被引擎兜底并入 primary 正文,复发检测继续可用。
    assert!(
        merged.body.contains("[fp:edit|not found]"),
        "{}",
        merged.body
    );
    assert_eq!(
        store.find_by_marker("[fp:edit|not found]").map(|e| e.id),
        Some(a.id.clone()),
    );
    // refs 并集进 primary。
    assert_eq!(merged.refs(), vec!["R-001".to_string()]);
    assert_eq!(
        merged.areas(),
        vec!["app/frontend", "engine/ipc"],
        "合并不得丢掉跨模块关系"
    );
    // R-165 批3:墓碑语义不变,但条目归档到 archive/。
    assert!(store.load_all().iter().all(|(_, e)| e.id != b.id));
    let dup_path = std::fs::read_dir(store.archive_dir())
        .unwrap()
        .flatten()
        .find(|p| {
            p.path()
                .file_stem()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with(&b.id))
                .unwrap_or(false)
        })
        .expect("archive 墓碑应存在")
        .path();
    let dup_text = std::fs::read_to_string(dup_path).unwrap();
    assert!(dup_text.contains("status: deprecated"), "{dup_text}");
    std::fs::remove_dir_all(dir).ok();
}

/// D-341 验收②:轮末自动处置机制测试——满足条件的 candidate 自动 promote,
/// 超期未处置自动 deprecated,未满足条件的不动;存量前后计数(文件+索引)可审计。
#[test]
fn reconcile_candidates_auto_promote_deprecate_and_keep() {
    let (dir, store) = temp_store();
    // ① 可晋升 candidate:带指纹 + 复发≥3 + 真实 episode(provenance 硬约束)。
    let fp = format!("[fp:edit|reconcile #{}-{}]", std::process::id(), "r");
    store
        .append_note("fixture source", &fp, "sop", &[])
        .unwrap();
    for _ in 0..3 {
        store.bump_recurrence(&fp);
    }
    let AddOutcome::Added(promotable) = store
        .add(
            "sop",
            "reconcile 可晋升",
            "自动晋升钩子",
            &format!("正文 {fp}"),
            "memory-manager",
            &[],
            None,
            true,
        )
        .unwrap()
    else {
        panic!("expected Added")
    };
    assert_eq!(promotable.status, "candidate");
    // ② 超期 candidate:手写 30 天前的文件(updated 远早于 max_age_days)。
    let old_name = "M-990-超期候选.md";
    std::fs::write(
        store.root.join(old_name),
        "---\nid: M-990\nscope: project\ncategory: sop\ntitle: reconcile 超期\ndescription: 超期钩子\nstatus: candidate\ncreated: 2026-07-01\nupdated: 2026-07-01\nsource: memory-manager\n---\n\n正文",
    )
    .unwrap();
    // ③ 未达标 candidate:无指纹、updated 今天 → 不动。
    let AddOutcome::Added(keep) = store
        .add(
            "sop",
            "reconcile 未达标",
            "未达标钩子",
            "正文无指纹",
            "memory-manager",
            &[],
            None,
            true,
        )
        .unwrap()
    else {
        panic!("expected Added")
    };
    assert_eq!(keep.status, "candidate");
    // 手写文件进 FTS,让索引计数与文件计数一致(存量 before 可审计)。
    store.refresh_derived().unwrap();
    let eid = crate::memory::seed_episode(&dir, "ses");
    assert!(
        store
            .promote(&promotable.id, &[(eid, None, None)], None)
            .is_err(),
        "仅复发和 episode 不得晋升"
    );
    let unverified = store.reconcile_candidates(Some(eid), 365).unwrap();
    assert!(unverified.promoted.is_empty());
    assert!(unverified.untouched.contains(&promotable.id));
    crate::memory::seed_entry_recovery(&store, eid, &promotable.id);
    let report = store.reconcile_candidates(Some(eid), 14).unwrap();
    let promotable_id = promotable.id.clone();
    let keep_id = keep.id.clone();
    assert_eq!(
        report.promoted,
        vec![promotable_id.clone()],
        "复发≥3 + 对应恢复证据应自动晋升"
    );
    assert_eq!(report.deprecated, vec!["M-990"], "超期未处置应自动清退");
    assert_eq!(
        report.untouched,
        vec![keep_id.clone()],
        "未达标应保持 candidate"
    );
    assert_eq!(report.candidate_files_before, 3, "存量 3 条 candidate 文件");
    assert_eq!(
        report.candidate_files_after, 1,
        "promote + deprecated 后只剩未达标 1 条"
    );
    assert_eq!(report.candidate_index_before, 3, "索引与文件一致(before)");
    assert_eq!(report.candidate_index_after, 1, "索引与文件一致(after)");
    // 状态落地:promote → active(主目录文件仍在);deprecated → 归档(主目录无文件)。
    let (_, p) = store
        .load_all()
        .into_iter()
        .find(|(_, e)| e.id == promotable_id)
        .unwrap();
    assert_eq!(p.status, "active", "晋升条目必须落 active");
    assert!(!store.root.join(old_name).exists(), "超期文件应移出主目录");
    assert!(
        store.load_archived_ids().contains(&"M-990".to_string()),
        "归档 ID 保留防复用"
    );
    // 不满足条件的不动:keep 仍是 candidate、文件仍在主目录(未验证不注入边界不变)。
    let (_, k) = store
        .load_all()
        .into_iter()
        .find(|(_, e)| e.id == keep_id)
        .unwrap();
    assert_eq!(k.status, "candidate", "未达标条目必须保持 candidate");
    assert!(
        store.root.join(format!("{}.md", keep.file_stem())).exists(),
        "未达标条目文件必须留在主目录"
    );
    std::fs::remove_dir_all(dir).ok();
}

/// R-295：候选超过健康水位时低价值优先清退，且清退走归档而非裸删。
#[test]
fn reconcile_candidates_capacity_retires_low_value_first() {
    let (dir, store) = temp_store();
    for index in 0..crate::memory::CANDIDATE_MAX_COUNT {
        let fingerprint = format!("[fp:capacity-{}-{}]", std::process::id(), index);
        store
            .append_note("capacity fixture source", &fingerprint, "fact", &[])
            .unwrap();
        store.bump_recurrence(&fingerprint);
        let AddOutcome::Added(entry) = store
            .add(
                "fact",
                &format!("capacity 高价值 {index}"),
                &format!("capacity 高价值钩子 {index}"),
                &format!("正文 {fingerprint}"),
                "memory-manager",
                &[],
                None,
                true,
            )
            .unwrap()
        else {
            panic!("expected high-value candidate")
        };
        assert_eq!(entry.status, "candidate");
    }
    let mut low_value_ids = Vec::new();
    for index in 0..6 {
        let AddOutcome::Added(entry) = store
            .add(
                "fact",
                &format!("capacity 低价值 {index}"),
                &format!("capacity 低价值钩子 {index}"),
                "正文无 fingerprint 且无 recurrence",
                "memory-manager",
                &[],
                None,
                true,
            )
            .unwrap()
        else {
            panic!("expected low-value candidate")
        };
        low_value_ids.push(entry.id);
    }
    store.refresh_derived().unwrap();
    let report = store.reconcile_candidates(None, 365).unwrap();
    assert_eq!(report.candidate_files_before, 30);
    assert_eq!(report.candidate_index_before, 30);
    assert_eq!(
        report.candidate_files_after,
        crate::memory::CANDIDATE_MAX_COUNT
    );
    assert_eq!(
        report.candidate_index_after,
        crate::memory::CANDIDATE_MAX_COUNT
    );
    assert_eq!(report.deprecated, low_value_ids);
    assert_eq!(
        report.untouched.len(),
        crate::memory::CANDIDATE_MAX_COUNT,
        "untouched 只保留最终仍为 candidate 的条目(容量出口清退的不再算 untouched)"
    );
    for id in low_value_ids {
        assert!(
            store.has_archived_id(&id),
            "低价值 candidate 必须保留归档墓碑: {id}"
        );
    }
    std::fs::remove_dir_all(dir).ok();
}
