#[tokio::test]
async fn tracker_normal_archive_records_each_successful_path_once() {
    let (dir, store, tool, ctx) = m6_tracker_fixture("archive-normal-receipts");
    let mut terminal = entry("R-001");
    terminal.status = "done".into();
    store.save(&[terminal]).unwrap();
    let output = tool.execute(json!({"action":"archive"}), &ctx).await;
    assert!(!output.is_error, "{}", output.content);
    assert!(output.content.contains("archived 1 terminal"));
    assert!(store.load().unwrap().is_empty());
    assert_eq!(store.load_archive().unwrap()[0].id, "R-001");
    let logs = crate::write_log::entries_after(&dir, 0).unwrap();
    assert_eq!(logs.len(), 2);
    for path in [&store.path, &store.archive_file()] {
        let relative = path
            .strip_prefix(&dir)
            .unwrap()
            .display()
            .to_string()
            .replace('\\', "/");
        let matching: Vec<_> = logs.iter().filter(|log| log.path == relative).collect();
        assert_eq!(
            matching.len(),
            1,
            "each normal path is recorded exactly once"
        );
        assert!(matching[0].matches_content(Some(&std::fs::read(path).unwrap())));
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn tracker_archive_field_cleanup_persists_without_new_terminal() {
    let (dir, store, tool, ctx) = m6_tracker_fixture("archive-cleanup");
    std::fs::write(store.archive_file(), "# Requirements Archive\n\n## R-002 history [done]\n- 说明: same\n- 说明: same\n- 验证: first\n- 验证: second\n").unwrap();
    let output = tool.execute(json!({"action":"archive"}), &ctx).await;
    assert!(!output.is_error, "{}", output.content);
    let reopened = DocStore::open(&dir, &REQUIREMENTS);
    let archived = reopened.load_archive().unwrap();
    assert_eq!(
        archived[0]
            .fields
            .iter()
            .filter(|(key, _)| key == "说明")
            .count(),
        1
    );
    assert_eq!(
        archived[0]
            .fields
            .iter()
            .filter(|(key, _)| key == "验证")
            .count(),
        2
    );
    assert_eq!(reopened.load().unwrap()[0].id, "R-001");
    let written = std::fs::read(reopened.archive_file()).unwrap();
    assert!(crate::write_log::entries_after(&dir, 0)
        .unwrap()
        .iter()
        .any(|log| log.path == ".kanzei/project/requirements-archive.md"
            && log.matches_content(Some(&written))));
    let output = tool.execute(json!({"action":"archive"}), &ctx).await;
    assert!(!output.is_error);
    assert_eq!(std::fs::read(reopened.archive_file()).unwrap(), written);
    std::fs::remove_dir_all(dir).unwrap();
}

/// R-227 验收②配套:archive_fill 通过 tracker 动作回填归档条目占位符。
#[tokio::test]
async fn archive_fill_回填归档占位符_缺参报错() {
    let dir = std::env::temp_dir().join(format!("kz-archive-fill-tool-{}", std::process::id()));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let store = DocStore::open(&dir, &REQUIREMENTS);
    // 走正常归档路径:活动条目标记 done → archive_terminal 落归档,满足完整性门禁。
    // 注意用 R-001 低位 id:UNACCOUNTED 检查是 1..=max 连续序列,R-198 会触发
    // R-001..R-197 缺失(execute 入口有完整性门禁;docstore 层测试可直接用 R-198)。
    let mut e = entry("R-001");
    e.status = "done".into();
    e.fields = vec![(
        "进展".into(),
        "全量 cargo test --workspace 全绿(T-1786565xxx,harness 118)".into(),
    )];
    store.save(&[e]).unwrap();
    store.archive_terminal().unwrap();
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };

    // 缺 old/new → 报错。
    let out = tool
        .execute(json!({"action": "archive_fill", "id": "R-001"}), &ctx)
        .await;
    assert!(out.is_error, "{}", out.content);
    assert!(out.content.contains("old"), "{}", out.content);

    // 正常回填。
    let out = tool
        .execute(
            json!({"action": "archive_fill", "id": "R-001",
                   "old": "T-1786565xxx", "new": "T-1786565346"}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "{}", out.content);
    assert!(out.content.contains("T-1786565346"), "{}", out.content);
    let archived = store.load_archive().unwrap();
    let r001 = archived.iter().find(|e| e.id == "R-001").unwrap();
    assert!(r001
        .fields
        .iter()
        .any(|(_, v)| v.contains("T-1786565346") && !v.contains("xxx")));

    // 找不到占位符 → 报错。
    let out = tool
        .execute(
            json!({"action": "archive_fill", "id": "R-001",
                   "old": "T-9999999999xxx", "new": "T-1786565346"}),
            &ctx,
        )
        .await;
    assert!(out.is_error, "{}", out.content);
    std::fs::remove_dir_all(dir).ok();
}

/// R-201 端到端:raw_lines 列出游离行(raw_delete 的标识来源),raw_delete
/// 删除单条后字段体系与文件其余字节不受影响。
#[tokio::test]
async fn raw_lines_raw_delete_清理游离行且字段不受影响() {
    let dir = std::env::temp_dir().join(format!(
        "kz-rawlines-tool-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let path = dir.join(REQUIREMENTS.rel_path);
    // 直接手写带游离行的文档:引擎渲染路径不会产生游离行,必须从历史文件形态进入。
    std::fs::write(
        &path,
        "\
# Requirements

## R-001 条目 [todo]
- 进展: 第一行

历史手写段落一
- 验收: 有验收
",
    )
    .unwrap();
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };

    // ①列出:输出带 [n] 序号 + 原文,同时给删除指引。
    let listed = tool
        .execute(json!({"action": "raw_lines", "id": "R-001"}), &ctx)
        .await;
    assert!(!listed.is_error, "{}", listed.content);
    assert!(listed.content.contains("[ 1]"), "{}", listed.content);
    assert!(
        listed.content.contains("历史手写段落一"),
        "{}",
        listed.content
    );
    assert!(
        listed.content.contains("raw_delete"),
        "输出要指明删除动作: {}",
        listed.content
    );

    // 未知 ID:拒绝并给出可用 ID。
    let missing = tool
        .execute(json!({"action": "raw_lines", "id": "R-999"}), &ctx)
        .await;
    assert!(missing.is_error, "{}", missing.content);

    // ②删除第 1 条:文件里只少那一行,字段一行不动。
    let deleted = tool
        .execute(
            json!({"action": "raw_delete", "id": "R-001", "ordinal": 1}),
            &ctx,
        )
        .await;
    assert!(!deleted.is_error, "{}", deleted.content);
    assert!(
        deleted.content.contains("已删除 R-001 的第 1 条游离行"),
        "{}",
        deleted.content
    );
    let after = std::fs::read_to_string(&path).unwrap();
    assert!(!after.contains("历史手写段落一"), "{after}");
    assert!(after.contains("## R-001 条目 [todo]"), "{after}");
    assert!(after.contains("- 进展: 第一行"), "{after}");
    assert!(after.contains("- 验收: 有验收"), "{after}");

    // ④字段解析不受影响。
    let store = DocStore::open(&dir, &REQUIREMENTS);
    let parsed = store.load().unwrap();
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].fields.len(), 2, "{:?}", parsed[0].fields);
    assert!(parsed[0]
        .fields
        .iter()
        .any(|(k, v)| k == "进展" && v == "第一行"));

    // 缺少 ordinal:raw_delete 拒绝。
    let no_ordinal = tool
        .execute(json!({"action": "raw_delete", "id": "R-001"}), &ctx)
        .await;
    assert!(no_ordinal.is_error, "{}", no_ordinal.content);

    std::fs::remove_dir_all(dir).ok();
}

/// normalize 必须清掉条目里的机制产物字段(`取活依据`)。
///
/// 它是调度器每轮重算的东西,却被写进条目;而 structured_entry 会把条目的**全部**
/// fields 序列化进控制状态,于是这行必然过期的快照跟着条目进每一次 work next。
/// dry-run 只报告、apply 才落盘,且**不写进展**——机制产物没有可审计价值,
/// 留一句「曾经写过」只是把噪音换个地方放。
#[tokio::test]
async fn normalize_清掉取活依据且不写进展() {
    let dir = std::env::temp_dir().join(format!(
        "kz-normalize-engine-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let mut item = entry("R-001");
    item.status = "doing".into();
    item.fields = vec![
        (
            "取活依据".into(),
            "engine:无可执行 WIP，按 defect-first 选队首".into(),
        ),
        ("进展".into(), "批次 1/2".into()),
    ];
    DocStore::open(&dir, &REQUIREMENTS).save(&[item]).unwrap();
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };

    // dry-run:只报告,不落盘。
    let dry = tool.execute(json!({"action": "normalize"}), &ctx).await;
    assert!(!dry.is_error, "{}", dry.content);
    assert!(dry.content.contains("engine-derived"), "{}", dry.content);
    let after_dry = DocStore::open(&dir, &REQUIREMENTS).load().unwrap();
    assert!(
        after_dry[0].fields.iter().any(|(k, _)| k == "取活依据"),
        "dry-run 不得落盘"
    );

    // apply:字段消失,进展原样不动。
    let applied = tool
        .execute(json!({"action": "normalize", "apply": true}), &ctx)
        .await;
    assert!(!applied.is_error, "{}", applied.content);
    let saved = DocStore::open(&dir, &REQUIREMENTS).load().unwrap();
    assert!(
        !saved[0].fields.iter().any(|(k, _)| k == "取活依据"),
        "apply 必须清掉机制产物字段: {:?}",
        saved[0].fields
    );
    let progress = saved[0]
        .fields
        .iter()
        .find(|(k, _)| k == "进展")
        .map(|(_, v)| v.as_str())
        .unwrap_or_default();
    assert_eq!(progress, "批次 1/2", "机制产物没有可审计价值,不得写进展");
    std::fs::remove_dir_all(dir).ok();
}

/// D-331 验收②:reopen/update 命中归档 ID 时不再报 unknown id,而是明确 archived
/// 且该动作不适用——agent 不会误以为 ID 不存在而绕过专用工具手改托管文档。
#[tokio::test]
async fn archived_id_reports_archived_not_unknown() {
    let dir = std::env::temp_dir().join(format!("kz-archived-{}", std::process::id()));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    let tool = TrackerTool {
        tool_name: "defect",
        noun: "defect",
        kind: &DEFECTS,
        requires_refs: None,
    };
    // 归档里放一个终态条目,活动文件为空。
    let mut e = entry("D-001");
    e.status = "fixed".into();
    e.title = "已归档缺陷".into();
    let store = DocStore::open(&dir, &DEFECTS);
    store.save(&[e]).unwrap();
    store.archive_terminal().unwrap();
    assert!(store.load().unwrap().is_empty(), "活动文件应为空");
    assert_eq!(store.load_archive().unwrap()[0].id, "D-001");
    // reopen 归档 ID:不再 unknown id,而是 archived + 指向纠错动作。
    let out = tool
        .execute(
            json!({"action": "reopen", "id": "D-001", "reason": "想拉回来"}),
            &ctx,
        )
        .await;
    assert!(out.is_error, "归档条目 reopen 应被拒: {}", out.content);
    assert!(out.content.contains("archived"), "{}", out.content);
    assert!(!out.content.contains("unknown id"), "{}", out.content);
    assert!(out.content.contains("fix_terminal"), "{}", out.content);
    // update 归档 ID 同理。
    let out = tool
        .execute(
            json!({"action": "update", "id": "D-001", "fields": {"进展": "x"}}),
            &ctx,
        )
        .await;
    assert!(out.is_error, "归档条目 update 应被拒: {}", out.content);
    assert!(out.content.contains("archived"), "{}", out.content);
    // 真不存在的 ID 仍报 unknown id(回归⑤)。
    let out = tool
        .execute(
            json!({"action": "reopen", "id": "D-999", "reason": "x"}),
            &ctx,
        )
        .await;
    assert!(out.content.contains("unknown id"), "{}", out.content);
    std::fs::remove_dir_all(dir).ok();
}

/// D-331 验收③④:fix_terminal 把归档终态从 fixed 纠为 wontfix,保持归档、
/// 清除标题里的跨 DocKind 状态标记污染(D-267 的 [dropped])、进展留审计;
/// 非法终态/缺理由/归档无此 ID 都拒绝。
#[tokio::test]
async fn fix_terminal_corrects_archived_status_and_strips_title_marker() {
    let dir = std::env::temp_dir().join(format!("kz-fix-terminal-{}", std::process::id()));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    let tool = TrackerTool {
        tool_name: "defect",
        noun: "defect",
        kind: &DEFECTS,
        requires_refs: None,
    };
    // 归档一个 fixed 条目,标题带 [dropped] 污染(复现 D-267 形态)。
    let mut e = entry("D-001");
    e.status = "fixed".into();
    e.title = "某缺陷 [dropped] 标题".into();
    let store = DocStore::open(&dir, &DEFECTS);
    store.save(&[e]).unwrap();
    store.archive_terminal().unwrap();
    // 纠为 wontfix。
    let out = tool
        .execute(
            json!({"action": "fix_terminal", "id": "D-001", "status": "wontfix",
                   "reason": "用户 2026-08-11 定调不做中间档,应记 wontfix 而非 fixed"}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "{}", out.content);
    assert!(
        out.content.contains("fixed") && out.content.contains("wontfix"),
        "{}",
        out.content
    );
    // 条目仍在归档、状态已改、标题标记被清、进展留审计。
    let archived = store.load_archive().unwrap();
    let entry = archived
        .iter()
        .find(|x| x.id == "D-001")
        .expect("条目应留在归档");
    assert_eq!(entry.status, "wontfix");
    assert!(
        !entry.title.contains("[dropped]"),
        "标题状态标记应被清除: {}",
        entry.title
    );
    assert!(
        entry.title.contains("某缺陷") && entry.title.contains("标题"),
        "其余标题逐字保留: {}",
        entry.title
    );
    assert!(
        entry.fields.iter().any(|(k, v)| k == "进展"
            && v.contains("[terminal-fix")
            && v.contains("fixed")
            && v.contains("wontfix")),
        "进展应留纠错审计: {:?}",
        entry.fields
    );
    assert!(store.load().unwrap().is_empty(), "活动文件不应出现该条目");
    // 非法终态(open 不是终态)拒绝。
    let out = tool
        .execute(
            json!({"action": "fix_terminal", "id": "D-001", "status": "open", "reason": "想退回"}),
            &ctx,
        )
        .await;
    assert!(out.is_error, "非终态不得作为纠错目标: {}", out.content);
    assert!(out.content.contains("terminal"), "{}", out.content);
    // 缺理由拒绝。
    let out = tool
        .execute(
            json!({"action": "fix_terminal", "id": "D-001", "status": "fixed"}),
            &ctx,
        )
        .await;
    assert!(out.is_error, "纠错必须给理由: {}", out.content);
    // 归档里不存在的 ID 拒绝。
    let out = tool
        .execute(
            json!({"action": "fix_terminal", "id": "D-999", "status": "fixed", "reason": "x x x"}),
            &ctx,
        )
        .await;
    assert!(out.is_error, "归档无此 ID 应拒绝: {}", out.content);
    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
async fn normalize_dry_run_reports_and_apply_fixes() {
    // D-332 验收②:normalize 是统一 repair surface——dry-run 只报告,
    // apply 机械修复(重复字段去重、标题标记剥离、显式 status 修正非法 lifecycle),
    // 幂等(重复 apply 无新变化)。
    let dir = std::env::temp_dir().join(format!(
        "kz-normalize-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };
    // 构造污染:R-001 非法 lifecycle [open] + 标题带 [done] 标记 + 重复「优先级」字段;
    // R-002 合法 todo。
    let e1 = Entry {
        id: "R-001".into(),
        title: "某需求 [done] 标题".into(),
        status: "open".into(), // requirement 无 open,非法
        severity: None,
        fields: vec![
            ("优先级".into(), "P1".into()),
            ("优先级".into(), "P2".into()), // 重复
            ("标签".into(), "核心".into()),
        ],
    };
    let e2 = Entry {
        id: "R-002".into(),
        title: "正常需求".into(),
        status: "todo".into(),
        severity: None,
        fields: vec![("优先级".into(), "P0".into())],
    };
    let store = DocStore::open(&dir, &REQUIREMENTS);
    store.save(&[e1, e2]).unwrap();

    // dry-run:只报告,不写入。
    let out = tool.execute(json!({"action": "normalize"}), &ctx).await;
    assert!(!out.is_error, "{}", out.content);
    assert!(out.content.contains("dry-run"), "{}", out.content);
    assert!(out.content.contains("R-001"), "{}", out.content);
    assert!(
        out.content.contains("invalid lifecycle `open`"),
        "{}",
        out.content
    );
    assert!(
        out.content.contains("duplicate field `优先级`"),
        "{}",
        out.content
    );
    assert!(
        out.content.contains("title status marker `[done]`"),
        "{}",
        out.content
    );
    // dry-run 不写盘
    let loaded = store.load().unwrap();
    assert_eq!(
        loaded[0].status, "open",
        "dry-run 不得改状态: {:?}",
        loaded[0].status
    );
    assert_eq!(
        loaded[0].fields.len(),
        3,
        "dry-run 不得去重: {:?}",
        loaded[0].fields
    );

    // apply + status 修正:非法 lifecycle → todo,标题标记剥离,重复字段去重。
    let out = tool
        .execute(
            json!({"action": "normalize", "apply": true, "status": "todo"}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "{}", out.content);
    assert!(out.content.contains("apply"), "{}", out.content);
    assert!(
        out.content.contains("lifecycle `open` → `todo`"),
        "{}",
        out.content
    );
    assert!(out.content.contains("deduplicated"), "{}", out.content);
    assert!(
        out.content.contains("stripped title status marker"),
        "{}",
        out.content
    );

    let loaded = store.load().unwrap();
    let r1 = loaded.iter().find(|x| x.id == "R-001").unwrap();
    assert_eq!(r1.status, "todo", "非法 lifecycle 应被修正: {}", r1.status);
    assert!(
        !r1.title.contains("[done]"),
        "标题标记应被剥离: {}",
        r1.title
    );
    assert_eq!(r1.title, "某需求 标题", "标题应保留其余文字: {}", r1.title);
    let prio: Vec<_> = r1.fields.iter().filter(|(k, _)| k == "优先级").collect();
    assert_eq!(prio.len(), 1, "重复字段应去重: {:?}", r1.fields);

    // 幂等:再次 dry-run 应无待修项。
    let out = tool.execute(json!({"action": "normalize"}), &ctx).await;
    assert!(!out.is_error, "{}", out.content);
    assert!(
        out.content.contains("无待修项") || !out.content.contains("R-001"),
        "{}",
        out.content
    );

    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
async fn d713_status_field_conflicts_are_gated_and_normalized_for_regression_ids() {
    let dir = std::env::temp_dir().join(format!(
        "kz-status-source-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };
    let store = DocStore::open(&dir, &REQUIREMENTS);
    let ids = ["R-284", "R-249", "R-264", "R-101"];
    let entries: Vec<_> = ids
        .iter()
        .map(|id| Entry {
            id: (*id).into(),
            title: format!("回归 {id}"),
            status: "doing".into(),
            severity: None,
            fields: vec![
                ("状态".into(), "todo".into()),
                ("进展".into(), "既有进展".into()),
            ],
        })
        .collect();
    store.save(&entries).unwrap();

    std::fs::write(
        store.archive_file(),
        "# Requirements Archive\n\n## R-101 归档回归 [done]\n- 状态: doing\n- 进展: 归档旧记录\n",
    )
    .unwrap();

    let issues = store.integrity_issues(&store.load().unwrap()).unwrap();
    for id in ids {
        assert!(
            issues
                .iter()
                .any(|issue| issue.contains(id) && issue.contains("conflicts")),
            "{id} 的正文状态冲突必须进入完整性门禁: {issues:?}"
        );
    }

    let rejected = tool
        .execute(
            json!({"action": "update", "id": "R-284", "fields": {"进展": "不应写入"}}),
            &ctx,
        )
        .await;
    assert!(
        rejected.is_error,
        "冲突状态下普通写入必须失败: {}",
        rejected.content
    );
    assert!(rejected.content.contains("tracker integrity is broken"));

    let dry = tool.execute(json!({"action": "normalize"}), &ctx).await;
    assert!(!dry.is_error, "dry-run 仍应可诊断冲突: {}", dry.content);
    for id in ids {
        assert!(
            dry.content.contains(id),
            "dry-run 必须点名 {id}: {}",
            dry.content
        );
    }
    assert!(
        store
            .load()
            .unwrap()
            .iter()
            .all(|entry| entry.fields.iter().any(|(key, _)| key == "状态")),
        "dry-run 不得写入"
    );

    let fixed = tool
        .execute(json!({"action": "normalize", "apply": true}), &ctx)
        .await;
    assert!(!fixed.is_error, "apply 应能修复冲突: {}", fixed.content);
    let cleaned = store.load().unwrap();
    for id in ids {
        let entry = cleaned.iter().find(|entry| entry.id == id).unwrap();
        assert_eq!(entry.status, "doing");
        assert!(
            entry
                .fields
                .iter()
                .all(|(key, _)| key != "状态" && !key.eq_ignore_ascii_case("status")),
            "{id} 不得保留正文状态副本: {:?}",
            entry.fields
        );
        assert!(
            entry
                .fields
                .iter()
                .any(|(key, value)| key == "进展" && value.contains("状态对账")),
            "{id} 必须把旧状态移入进展: {:?}",
            entry.fields
        );
    }
    let archived = store.load_archive().unwrap();
    assert!(
        archived[0]
            .fields
            .iter()
            .all(|(key, _)| key != "状态" && !key.eq_ignore_ascii_case("status")),
        "归档区也不得保留正文状态副本: {:?}",
        archived[0].fields
    );
    assert!(
        archived[0]
            .fields
            .iter()
            .any(|(key, value)| key == "进展" && value.contains("状态对账")),
        "归档旧状态必须进入进展: {:?}",
        archived[0].fields
    );
    let again = tool.execute(json!({"action": "normalize"}), &ctx).await;
    assert!(
        !again.content.contains("reserved status field"),
        "apply 必须幂等: {}",
        again.content
    );
    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
async fn normalize_archived_non_terminal_reports_and_keeps_status() {
    // D-336:归档区非终态 lifecycle 仍只报告(修复通道是 fix_terminal 纠错),
    // apply 不改归档 status;归档**重复字段**由 apply 走 dedupe_archived_fields
    // 修复(见 dedupe_archived_fields_merges_progress_and_keeps_first_of_others)。
    let dir = std::env::temp_dir().join(format!(
        "kz-normalize-arch-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    let tool = TrackerTool {
        tool_name: "defect",
        noun: "defect",
        kind: &DEFECTS,
        requires_refs: None,
    };
    let store = DocStore::open(&dir, &DEFECTS);
    // 直接写归档文件,模拟手改产生的归档区非终态条目。
    std::fs::write(
        store.archive_file(),
        "# Defects\n\n## D-001 已修缺陷 [open] (medium)\n",
    )
    .unwrap();
    std::fs::write(store.path.clone(), "# Defects\n").unwrap();

    let out = tool
        .execute(json!({"action": "normalize", "apply": true}), &ctx)
        .await;
    assert!(!out.is_error, "{}", out.content);
    assert!(
        out.content.contains("archived D-001") && out.content.contains("non-terminal"),
        "归档区非终态应被报告: {}",
        out.content
    );
    // 归档非终态 status 不被 apply 改动(fix_terminal 才是纠错通道)
    let archived = store.load_archive().unwrap();
    assert_eq!(
        archived[0].status, "open",
        "apply 不得改动归档 status: {}",
        archived[0].status
    );

    std::fs::remove_dir_all(dir).ok();
}

/// D-358:apply 真去重了就得报出来。归档去重原先跑在输出拼装**之后**,
/// push 进 fixed 的条目一条也进不了 content——实测修了 6 条却报「0 fix(es)」、
/// 连「已修复」段都没有。工具少报自己的工作,在证据驱动的流程里等于说谎:
/// 上一轮正是照着这个输出把 D-333 验收③判成不可修、挂了个用户阻塞。
/// dry-run 的 findings 文案同步改成「apply 可自动收敛」,不再说「需手动整理归档」。
#[tokio::test]
async fn normalize_apply_如实报出归档去重条数() {
    let dir = std::env::temp_dir().join(format!(
        "kz-normalize-report-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    let tool = TrackerTool {
        tool_name: "defect",
        noun: "defect",
        kind: &DEFECTS,
        requires_refs: None,
    };
    let store = DocStore::open(&dir, &DEFECTS);
    // 归档条目带两份「进展」:D-330/D-331 修复前的存量形态。
    std::fs::write(
        store.archive_file(),
        "# Defects\n\n## D-001 已修缺陷 [fixed] (medium)\n\
             - 进展: 第一段证据\n- 进展: 第二段证据\n",
    )
    .unwrap();
    std::fs::write(store.path.clone(), "# Defects\n").unwrap();

    // dry-run:报告要指向 apply 可修,不能再说「需手动整理归档」。
    let dry = tool.execute(json!({"action": "normalize"}), &ctx).await;
    assert!(!dry.is_error, "{}", dry.content);
    assert!(
        dry.content.contains("apply 可自动收敛") && !dry.content.contains("需手动整理归档"),
        "dry-run 文案不得否认 apply 的能力: {}",
        dry.content
    );

    // apply:计数与「已修复」段都要如实带上归档去重。
    let out = tool
        .execute(json!({"action": "normalize", "apply": true}), &ctx)
        .await;
    assert!(!out.is_error, "{}", out.content);
    assert!(
        !out.content.contains("0 fix(es)"),
        "真去重了就不能报 0 fix(es): {}",
        out.content
    );
    assert!(
        out.content.contains("已修复") && out.content.contains("D-001"),
        "「已修复」段必须列出被去重的归档条目: {}",
        out.content
    );
    // 复查:重复字段真的收敛了(报告与事实一致,不是只改了输出)。
    let again = tool.execute(json!({"action": "normalize"}), &ctx).await;
    assert!(
        !again.content.contains("duplicate field"),
        "apply 之后不该再有重复字段: {}",
        again.content
    );

    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
async fn reorder_rewrites_file_order_and_rejects_partial() {
    let dir = std::env::temp_dir().join(format!("kz-reorder-{}", std::process::id()));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let store = DocStore::open(&dir, &REQUIREMENTS);
    store
        .save(&[entry("R-001"), entry("R-002"), entry("R-003")])
        .unwrap();
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };
    let ctx = ToolCtx::new(dir.clone(), dir.clone());

    // 完整置换:成功且文件顺序改变。
    let out = tool
        .execute(
            json!({"action": "reorder", "order": ["R-003", "R-001", "R-002"]}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "{}", out.content);
    let ids: Vec<String> = store.load().unwrap().iter().map(|e| e.id.clone()).collect();
    assert_eq!(ids, vec!["R-003", "R-001", "R-002"]);

    // 不完整清单:拒绝且顺序不变。
    let out = tool
        .execute(json!({"action": "reorder", "order": ["R-001"]}), &ctx)
        .await;
    assert!(out.is_error);
    let ids: Vec<String> = store.load().unwrap().iter().map(|e| e.id.clone()).collect();
    assert_eq!(ids, vec!["R-003", "R-001", "R-002"]);
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn add_preserves_handwritten_free_text_and_unknown_blocks() {
    let dir = std::env::temp_dir().join(format!("kz-preserve-{}", std::process::id()));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let path = dir.join(REQUIREMENTS.rel_path);
    std::fs::write(
        &path,
        "# Requirements\n\n手写说明: 不应被引擎删除\n- 就是个备注\n\n## R-001 已有条目 [todo]\n### 子标题\n```text\n用户代码块\n```\n- 验收: 原字段\n",
    )
    .unwrap();
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };
    let output = tool
        .execute(
            json!({"action": "add", "title": "新条目", "priority": "P2", "fields": {"复杂度": "中", "标签": "后端", "来源": "用户原话「测试」", "发现记录": "{\"Intent\":\"测试意图\",\"Explicit\":\"用户原话\",\"Assumptions\":\"无\",\"Ambiguities\":\"无\",\"领域对象\":\"条目\",\"最小成功闭环\":\"登记\",\"延后决策\":\"无\"}"}}),
            &ToolCtx::new(dir.clone(), dir.clone()),
        )
        .await;
    assert!(!output.is_error, "{}", output.content);
    let saved = std::fs::read_to_string(path).unwrap();
    for line in [
        "手写说明: 不应被引擎删除",
        "- 就是个备注",
        "### 子标题",
        "用户代码块",
    ] {
        assert!(saved.contains(line), "missing preserved line: {line}");
    }
    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
async fn archive_preserves_handwritten_free_text_and_unknown_blocks() {
    let dir = std::env::temp_dir().join(format!("kz-archive-preserve-{}", std::process::id()));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let path = dir.join(REQUIREMENTS.rel_path);
    std::fs::write(
        &path,
        "# Requirements\n\n## R-001 已完成条目 [done]\n终态说明: 归档时也不能丢\n- 手写备注\n### 归档子标题\n```text\n归档代码块\n```\n- 验收: 原字段\n\n## R-002 进行中条目 [doing]\n- 验收: 保留在活动文档\n",
    )
    .unwrap();
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };
    let output = tool
        .execute(
            json!({"action": "archive"}),
            &ToolCtx::new(dir.clone(), dir.clone()),
        )
        .await;
    assert!(!output.is_error, "{}", output.content);

    let archive =
        std::fs::read_to_string(dir.join(".kanzei/project/requirements-archive.md")).unwrap();
    for line in [
        "终态说明: 归档时也不能丢",
        "- 手写备注",
        "### 归档子标题",
        "归档代码块",
    ] {
        assert!(archive.contains(line), "missing preserved line: {line}");
    }
    let active = std::fs::read_to_string(path).unwrap();
    assert!(!active.contains("终态说明: 归档时也不能丢"));
    assert!(active.contains("## R-002 进行中条目 [doing]"));
    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
async fn archive_reports_moved_ids_and_requires_paired_commit() {
    // D-112:归档输出必须列出移动的 ID 并要求两文件同一提交。
    let dir = std::env::temp_dir().join(format!(
        "kz-tracker-archive-msg-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let store = DocStore::open(&dir, &REQUIREMENTS);
    let mut done = entry("R-001");
    done.status = "done".into();
    store.save(&[done, entry("R-002")]).unwrap();
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };
    let out = tool
        .execute(
            json!({"action": "archive"}),
            &ToolCtx::new(dir.clone(), dir.clone()),
        )
        .await;
    assert!(!out.is_error, "{}", out.content);
    assert!(out.content.contains("R-001"), "{}", out.content);
    assert!(out.content.contains("SAME commit"), "{}", out.content);
    std::fs::remove_dir_all(&dir).ok();
}

/// D-398:tracker archive 动作写活动+归档两个文件——都记写日志
/// (归档侧不记则新增被围栏回滚,条目从两份同时消失 D-112)。
#[tokio::test]
async fn archive_写日志含活动与归档文件() {
    let dir = std::env::temp_dir().join(format!("kz-arch-log-{}", std::process::id()));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let store = DocStore::open(&dir, &REQUIREMENTS);
    let mut done = entry("R-001");
    done.status = "done".into();
    store.save(&[done, entry("R-002")]).unwrap();
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };
    let out = tool
        .execute(
            json!({"action": "archive"}),
            &ToolCtx::new(dir.clone(), dir.clone()),
        )
        .await;
    assert!(!out.is_error, "{}", out.content);
    let logs = crate::write_log::entries_after(&dir, 0).unwrap();
    let active_rel = store
        .path
        .strip_prefix(&dir)
        .unwrap()
        .display()
        .to_string()
        .replace('\\', "/");
    let archive_rel = store
        .archive_file()
        .strip_prefix(&dir)
        .unwrap()
        .display()
        .to_string()
        .replace('\\', "/");
    let paths: Vec<String> = logs.iter().map(|l| l.path.clone()).collect();
    assert!(
        paths.iter().any(|p| p.ends_with(&active_rel)),
        "活动文件应有写日志: {paths:?}"
    );
    assert!(
        paths.iter().any(|p| p.ends_with(&archive_rel)),
        "归档文件应有写日志(否则归档侧新增被回滚): {paths:?}"
    );
    std::fs::remove_dir_all(&dir).ok();
}
