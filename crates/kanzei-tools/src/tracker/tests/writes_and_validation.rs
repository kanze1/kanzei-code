/// D-330:add/repair_missing_id 时 priority 参数与 fields 里「优先级」键只落一条——
/// 同传两者不再双写同名字段(值不同语义歧义),参数优先覆盖(fields 里值被顶掉)。
#[tokio::test]
async fn add_and_repair_dedupe_priority_param_with_fields_key() {
    let dir = std::env::temp_dir().join(format!(
        "kz-add-prio-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let store = DocStore::open(&dir, &REQUIREMENTS);
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    // add:priority 参数 P1 + fields 里「优先级: P2」——只落一条,参数优先。
    let out = tool
        .execute(
            json!({"action": "add", "title": "双写优先级测试", "priority": "P1",
                   "fields": {"复杂度": "小", "优先级": "P2", "标签": "后端"}}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "{}", out.content);
    let entry = store
        .load()
        .unwrap()
        .into_iter()
        .find(|e| e.title == "双写优先级测试")
        .expect("条目应存在");
    let prio: Vec<_> = entry.fields.iter().filter(|(k, _)| k == "优先级").collect();
    assert_eq!(
        prio.len(),
        1,
        "add 优先级字段只应有一条: {:?}",
        entry.fields
    );
    assert_eq!(prio[0].1, "P1", "priority 参数应覆盖 fields 里的值");
    // repair_missing_id:同型——恢复缺失编号同时传两处优先级。
    let out2 = tool
        .execute(
            json!({"action": "repair_missing_id", "id": "R-002", "title": "恢复条目",
                   "priority": "P2", "fields": {"优先级": "P3", "标签": "后端"}}),
            &ctx,
        )
        .await;
    assert!(!out2.is_error, "{}", out2.content);
    let e2 = store
        .load()
        .unwrap()
        .into_iter()
        .find(|e| e.id == "R-002")
        .expect("恢复条目应存在");
    let prio2: Vec<_> = e2.fields.iter().filter(|(k, _)| k == "优先级").collect();
    assert_eq!(
        prio2.len(),
        1,
        "repair 优先级字段只应有一条: {:?}",
        e2.fields
    );
    assert_eq!(prio2[0].1, "P2", "repair 的 priority 参数应覆盖 fields 值");
    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
async fn requirement_defect_full_list_requires_explicit_dedup_reason() {
    let dir = std::env::temp_dir().join(format!(
        "kz-list-guard-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    DocStore::open(&dir, &REQUIREMENTS)
        .save(&[entry("R-001")])
        .unwrap();
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    let rejected = tool.execute(json!({"action": "list"}), &ctx).await;
    assert!(rejected.is_error);
    assert!(rejected.content.contains("work next"));
    let allowed = tool
        .execute(
            json!({"action": "list", "reason": "deduplicate_registration"}),
            &ctx,
        )
        .await;
    assert!(!allowed.is_error, "{}", allowed.content);
    let value: serde_json::Value = serde_json::from_str(&allowed.content).unwrap();
    assert_eq!(value["entries"][0]["id"], "R-001");
    std::fs::remove_dir_all(dir).ok();
}

/// R-184 批5 收活格5:append_progress 追加进展与锚点,不改状态/标题/业务字段。
#[test]
fn append_progress_only_appends_progress_field_and_keeps_state() {
    let dir = std::env::temp_dir().join(format!(
        "kz-append-progress-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let store = DocStore::open(&dir, &REQUIREMENTS);
    let mut e = entry("R-001");
    e.status = "doing".into();
    e.fields.push(("进展".into(), "2026-08-10 既有进展".into()));
    e.fields.push(("优先级".into(), "P1".into()));
    store.save(&[e]).unwrap();

    let updated = super::append_progress(&dir, &REQUIREMENTS, "R-001", "由 M 线交付合并").unwrap();
    assert_eq!(updated.status, "doing", "回写不得改状态");
    assert_eq!(updated.title, "t-R-001", "回写不得改标题");
    let progress = updated
        .fields
        .iter()
        .find(|(k, _)| k == "进展")
        .map(|(_, v)| v.as_str())
        .unwrap();
    assert!(progress.starts_with("2026-08-10 既有进展\n"), "{progress}");
    assert!(
        progress.ends_with("收活回写: 由 M 线交付合并"),
        "{progress}"
    );
    // 优先级字段原样保留。
    assert!(updated
        .fields
        .iter()
        .any(|(k, v)| k == "优先级" && v == "P1"));
    for anchor in ["recorded_at", "observed_head", "observed_worktree_hash"] {
        assert!(
            updated.fields.iter().any(|(key, _)| key == anchor),
            "缺进展 provenance 锚点 {anchor}: {:?}",
            updated.fields
        );
    }
    assert!(updated
        .fields
        .iter()
        .any(|(key, value)| key == "recorded_at" && value.parse::<u128>().is_ok()));
    assert!(updated
        .fields
        .iter()
        .any(|(key, value)| { key == "observed_worktree_hash" && value.starts_with("fnv1a64:") }));

    // 无既有进展字段的条目:直接新建该字段。保存必须带上 R-001,否则覆盖丢条目。
    let e2 = entry("R-002");
    let mut both = store.load().unwrap();
    both.push(e2);
    store.save(&both).unwrap();
    let updated2 = super::append_progress(&dir, &REQUIREMENTS, "R-002", "第二条").unwrap();
    let progress2 = updated2
        .fields
        .iter()
        .find(|(k, _)| k == "进展")
        .map(|(_, v)| v.as_str())
        .unwrap();
    assert!(progress2.starts_with("20"), "{progress2}");
    assert!(progress2.contains("收活回写: 第二条"), "{progress2}");

    // 未知 ID 拒绝且不写盘。
    let before = store.load().unwrap();
    let err = super::append_progress(&dir, &REQUIREMENTS, "R-999", "x").unwrap_err();
    assert!(err.contains("unknown id `R-999`"), "{err}");
    let after = store.load().unwrap();
    assert_eq!(after.len(), before.len(), "未知 ID 不得写盘");

    std::fs::remove_dir_all(dir).ok();
}

/// R-232 验收①:同值 update 返回 no-op 且文件字节不变。
/// 验收②:变更 update 返回 旧→新 摘要。
#[tokio::test]
async fn 同值update返回noop且文件字节不变_变更返回旧到新摘要() {
    let dir = std::env::temp_dir().join(format!("kz-idem-update-{}", std::process::id()));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let mut e = entry("R-001");
    e.status = "doing".into();
    e.fields = vec![
        ("优先级".into(), "P1".into()),
        ("进展".into(), "2026-08-10 既有".into()),
    ];
    DocStore::open(&dir, &REQUIREMENTS).save(&[e]).unwrap();
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };
    let path = dir.join(".kanzei/project/requirements.md");

    // 同值 update:no-op + 文件字节不变。
    let before_bytes = std::fs::read(&path).unwrap();
    let out = tool
        .execute(
            json!({"action": "update", "id": "R-001",
                   "fields": {"优先级": "P1", "进展": "2026-08-10 既有"}}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "{}", out.content);
    assert!(out.content.contains("no-op"), "{}", out.content);
    let after_bytes = std::fs::read(&path).unwrap();
    assert_eq!(before_bytes, after_bytes, "同值 update 必须零写入");

    // 变更 update:返回 旧→新 摘要且落盘。
    let out = tool
        .execute(
            json!({"action": "update", "id": "R-001", "fields": {"优先级": "P2"}}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "{}", out.content);
    assert!(out.content.contains("优先级: P1 → P2"), "{}", out.content);
    let after_bytes = std::fs::read(&path).unwrap();
    assert!(
        after_bytes.windows(b"P2".len()).any(|w| w == b"P2"),
        "变更必须落盘"
    );
    assert_ne!(before_bytes, after_bytes, "变更必须改变文件");

    // 变更后同值再 update:又是 no-op。
    let out = tool
        .execute(
            json!({"action": "update", "id": "R-001", "fields": {"优先级": "P2"}}),
            &ctx,
        )
        .await;
    assert!(out.content.contains("no-op"), "{}", out.content);
    std::fs::remove_dir_all(dir).ok();
}

/// D-276 端到端:update 传**多行**进展值时——①不新增游离段落(push_field 折行,
/// D-294 既有能力);②若条目已有历史游离段落,返回里自检告警并指路
/// raw_lines/raw_delete(修复方向③,本次交付);③raw_delete 清完后 update
/// 不再告警。
#[tokio::test]
async fn update多行值不新增游离段落且已有残留被自检点名() {
    let dir = std::env::temp_dir().join(format!(
        "kz-d276-update-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let path = dir.join(REQUIREMENTS.rel_path);
    // 手写带一条历史游离段落的文档(引擎渲染路径不会产生游离行)。
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

    // ①update 传多行进展值:第一段是首行,第二段会折行进同一字段(不新增游离段落)。
    let updated = tool
        .execute(
            json!({"action": "update", "id": "R-001",
                       "fields": {"进展": "第一段\n第二段\n第三段"}}),
            &ctx,
        )
        .await;
    assert!(!updated.is_error, "{}", updated.content);
    // 自检告警:仍有 1 条历史游离段落被点名 + 指路清理工具。
    assert!(
        updated.content.contains("游离段落"),
        "update 后应自检告警残留: {}",
        updated.content
    );
    assert!(
        updated.content.contains("raw_lines") && updated.content.contains("raw_delete"),
        "告警要指路清理通道: {}",
        updated.content
    );
    // 文件里多行值被折成单行,且游离段落数量不变(仍是那 1 条历史残留)。
    let after = std::fs::read_to_string(&path).unwrap();
    assert!(
        after.contains("- 进展: 第一段 第二段 第三段"),
        "多行值必须折成单行字段: {after}"
    );
    assert!(
        after.contains("历史手写段落一"),
        "历史游离段落仍存在(update 不新增也不清除): {after}"
    );
    assert!(
        !after.contains("第二段\n-"),
        "第二段不能变成新的游离段落: {after}"
    );

    // ②raw_delete 清掉历史残留后再 update:不再告警。
    let deleted = tool
        .execute(
            json!({"action": "raw_delete", "id": "R-001", "ordinal": 1}),
            &ctx,
        )
        .await;
    assert!(!deleted.is_error, "{}", deleted.content);
    let updated2 = tool
        .execute(
            json!({"action": "update", "id": "R-001",
                       "fields": {"进展": "只有一段"}}),
            &ctx,
        )
        .await;
    assert!(!updated2.is_error, "{}", updated2.content);
    assert!(
        !updated2.content.contains("游离段落"),
        "清完后 update 不应再告警: {}",
        updated2.content
    );

    std::fs::remove_dir_all(dir).ok();
}

/// D-331 验收①:add/update/repair_missing_id 拒绝标题里的跨 DocKind 状态标记
/// (`[dropped]`/`[done]` 等)——状态的家是 header 方括号,标题带标记会渲染成
/// 双终态污染(如 D-267 的 `[dropped] [fixed]`)。
#[tokio::test]
async fn title_status_marker_rejected_on_all_write_actions() {
    let dir = std::env::temp_dir().join(format!("kz-title-marker-{}", std::process::id()));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    let tool = TrackerTool {
        tool_name: "defect",
        noun: "defect",
        kind: &DEFECTS,
        requires_refs: None,
    };
    // add:dropped 是 requirements/findings 的状态,不是缺陷状态 → 标题不得携带。
    for title in ["某缺陷 [dropped]", "某缺陷 [stale]"] {
        let out = tool
            .execute(
                json!({"action": "add", "title": title, "priority": "P2", "severity": "medium",
                       "fields": {"复杂度": "小", "标签": "后端"}}),
                &ctx,
            )
            .await;
        assert!(out.is_error, "add 应拒绝标题状态标记: {}", out.content);
        assert!(out.content.contains("status marker"), "{}", out.content);
    }
    // update:改标题携带 [done] → 拒绝。
    let mut e = entry("D-001");
    e.status = "open".into();
    DocStore::open(&dir, &DEFECTS).save(&[e]).unwrap();
    let out = tool
        .execute(
            json!({"action": "update", "id": "D-001", "title": "完成 [done] 的标题"}),
            &ctx,
        )
        .await;
    assert!(out.is_error, "update 应拒绝标题状态标记: {}", out.content);
    assert!(out.content.contains("status marker"), "{}", out.content);
    // repair_missing_id 同型。
    let out = tool
        .execute(
            json!({"action": "repair_missing_id", "id": "D-002", "title": "恢复 [fixed]",
                   "priority": "P2", "fields": {"标签": "后端"}}),
            &ctx,
        )
        .await;
    assert!(
        out.is_error,
        "repair_missing_id 应拒绝标题状态标记: {}",
        out.content
    );
    assert!(out.content.contains("status marker"), "{}", out.content);
    // 合法标题照常放行(不带方括号状态标记)。
    let out = tool
        .execute(
            json!({"action": "add", "title": "正常标题", "priority": "P2", "severity": "medium",
                   "fields": {"复杂度": "小", "标签": "后端"}}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "合法标题应放行: {}", out.content);
    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
async fn bracketed_title_keeps_status_through_real_tracker_add_and_load() {
    let dir = std::env::temp_dir().join(format!(
        "kz-bracket-title-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    let tool = TrackerTool {
        tool_name: "idea",
        noun: "idea",
        kind: &kanzei_memory::docstore::IDEAS,
        requires_refs: None,
    };
    let out = tool
        .execute(json!({"action": "add", "title": "支持协议 [RFC]"}), &ctx)
        .await;
    assert!(!out.is_error, "合法标题应被工具接受: {}", out.content);
    let store = DocStore::open(&dir, &kanzei_memory::docstore::IDEAS);
    let entries = store.load().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].title, "支持协议 [RFC]");
    assert_eq!(entries[0].status, "inbox");
    assert!(store.integrity_issues(&entries).unwrap().is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn parentheses_title_keeps_severity_when_updating_a_statusless_entry() {
    let dir = std::env::temp_dir().join(format!(
        "kz-parens-title-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    let tool = TrackerTool {
        tool_name: "defect",
        noun: "defect",
        kind: &DEFECTS,
        requires_refs: None,
    };
    let store = DocStore::open(&dir, &DEFECTS);
    store
        .save(&[Entry {
            id: "D-001".into(),
            title: "旧标题".into(),
            status: String::new(),
            severity: Some("low".into()),
            fields: vec![],
        }])
        .unwrap();
    let out = tool
        .execute(
            json!({"action": "update", "id": "D-001", "title": "新标题 (high)"}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "合法标题更新应成功: {}", out.content);
    let entries = store.load().unwrap();
    assert_eq!(entries[0].title, "新标题 (high)");
    assert_eq!(entries[0].status, "");
    assert_eq!(entries[0].severity.as_deref(), Some("low"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn priority_update_reuses_existing_english_field() {
    let dir = std::env::temp_dir().join(format!("kz-priority-{}", std::process::id()));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let store = DocStore::open(&dir, &REQUIREMENTS);
    let mut item = entry("R-001");
    item.fields.push(("priority".into(), "P2".into()));
    store.save(&[item]).unwrap();
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };
    let output = tool
        .execute(
            json!({"action": "update", "id": "R-001", "priority": "P0"}),
            &ToolCtx::new(dir.clone(), dir.clone()),
        )
        .await;
    assert!(!output.is_error, "{}", output.content);
    let fields = &store.load().unwrap()[0].fields;
    assert_eq!(fields, &[("priority".into(), "P0".into())]);
    std::fs::remove_dir_all(&dir).ok();
}

// R-112:标签受控词表校验——add/update 时词表外拒绝并提示合法值,词表内放行;
// 无标签字段或不参与分类的文档不受影响。
#[tokio::test]
async fn tag_validation_rejects_out_of_vocabulary_on_add_and_update() {
    let dir = std::env::temp_dir().join(format!(
        "kz-tag-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let store = DocStore::open(&dir, &REQUIREMENTS);
    store.save(&[entry("R-001")]).unwrap();
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };
    let ctx = ToolCtx::new(dir.clone(), dir.clone());

    // add:词表外标签被拒,错误里带合法词表。
    let out = tool
        .execute(
            json!({"action": "add", "title": "t", "priority": "P2", "fields": {"复杂度": "中", "标签": "杂项"}}),
            &ctx,
        )
        .await;
    assert!(out.is_error, "{}", out.content);
    assert!(
        out.content.contains("invalid tag `杂项`"),
        "{}",
        out.content
    );
    assert!(out.content.contains("核心"), "{}", out.content);
    assert!(out.content.contains("后端"), "{}", out.content);

    // add:词表内标签放行。
    let out = tool
        .execute(
            json!({"action": "add", "title": "t", "priority": "P2", "fields": {"复杂度": "中", "标签": "前端", "来源": "用户原话「测试」", "发现记录": "{\"Intent\":\"测试意图\",\"Explicit\":\"用户原话\",\"Assumptions\":\"无\",\"Ambiguities\":\"无\",\"领域对象\":\"条目\",\"最小成功闭环\":\"登记\",\"延后决策\":\"无\"}"}}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "{}", out.content);

    // update:词表外标签被拒。
    let out = tool
        .execute(
            json!({"action": "update", "id": "R-001", "fields": {"标签": "网络"}}),
            &ctx,
        )
        .await;
    assert!(out.is_error, "{}", out.content);
    assert!(
        out.content.contains("invalid tag `网络`"),
        "{}",
        out.content
    );

    // update:多值含一个非法词也被拒(按空白/逗号拆分逐词校验)。
    let out = tool
        .execute(
            json!({"action": "update", "id": "R-001", "fields": {"标签": "核心 杂项"}}),
            &ctx,
        )
        .await;
    assert!(out.is_error, "{}", out.content);
    assert!(
        out.content.contains("invalid tag `杂项`"),
        "{}",
        out.content
    );

    // update:词表内多值放行。
    let out = tool
        .execute(
            json!({"action": "update", "id": "R-001", "fields": {"标签": "核心 流程"}}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "{}", out.content);

    // 无标签字段的更新不受影响(close 走 fields 合并,不应误伤)。
    let out = tool
        .execute(
            json!({"action": "update", "id": "R-001", "fields": {"进展": "x"}}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "{}", out.content);
    std::fs::remove_dir_all(dir).ok();
}

// R-191:登记硬约束——新建 req 缺 复杂度/优先级/标签 即拒并提示补什么,
// 新建 defect 缺 severity 即拒;补全后放行;idea(severities/priorities/tags
// 全 None)不受影响。跨项目一致性的机械门禁:登记缺字段不再静默放行。
#[tokio::test]
async fn add_requires_registration_fields() {
    let dir = std::env::temp_dir().join(format!(
        "kz-r191-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let ctx = ToolCtx::new(dir.clone(), dir.clone());

    // req:信息不足直接落 draft，不要求模型补写估计值。
    let req_tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };
    let out = req_tool
        .execute(json!({"action": "add", "title": "裸登记"}), &ctx)
        .await;
    assert!(!out.is_error, "{}", out.content);
    assert!(out.content.contains("[draft]"), "{}", out.content);

    // req:仅有估计信息仍是 draft。
    let out = req_tool
        .execute(
            json!({"action": "add", "title": "半裸", "fields": {"复杂度": "中"}}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "{}", out.content);
    assert!(out.content.contains("[draft]"), "{}", out.content);

    // req:字段补全 → 放行。
    let out = req_tool
        .execute(
            json!({"action": "add", "title": "完整", "priority": "P2",
                   "fields": {"复杂度": "中", "标签": "后端", "来源": "用户原话「测试」", "发现记录": "{\"Intent\":\"测试意图\",\"Explicit\":\"用户原话\",\"Assumptions\":\"无\",\"Ambiguities\":\"无\",\"领域对象\":\"条目\",\"最小成功闭环\":\"登记\",\"延后决策\":\"无\"}"}}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "{}", out.content);

    // schema 暴露的顶层字段必须直接可执行，不能再逼模型猜 fields 中文键。
    let out = req_tool
        .execute(
            json!({"action": "add", "title": "顶层字段", "priority": "P2",
                   "complexity": "小", "tag": "后端"}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "{}", out.content);
    let req_text = std::fs::read_to_string(dir.join(REQUIREMENTS.rel_path)).unwrap();
    assert!(req_text.contains("- 复杂度: 小"), "{req_text}");
    assert!(req_text.contains("- 标签: 后端"), "{req_text}");
    let invalid = req_tool
        .execute(
            json!({"action": "add", "title": "非法复杂度", "priority": "P2",
                   "complexity": "巨大", "tag": "后端"}),
            &ctx,
        )
        .await;
    assert!(invalid.is_error);
    assert!(
        invalid.content.contains("valid: 小 | 中 | 大"),
        "{}",
        invalid.content
    );

    // defect:缺 severity → 拒绝;补全 → 放行。
    let def_tool = TrackerTool {
        tool_name: "defect",
        noun: "defect",
        kind: &DEFECTS,
        requires_refs: None,
    };
    let out = def_tool
        .execute(
            json!({"action": "add", "title": "缺陷裸登记", "priority": "P2",
                   "fields": {"标签": "前端"}}),
            &ctx,
        )
        .await;
    assert!(
        out.is_error,
        "缺 severity 的 defect add 必须被拒: {}",
        out.content
    );
    assert!(out.content.contains("severity"), "{}", out.content);
    let out = def_tool
        .execute(
            json!({"action": "add", "title": "缺陷完整", "severity": "medium",
                   "priority": "P2", "fields": {"标签": "前端"}}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "{}", out.content);

    // idea(无必填 kind 字段):裸 add 不受影响。
    let idea_tool = TrackerTool {
        tool_name: "idea",
        noun: "idea",
        kind: &IDEAS,
        requires_refs: None,
    };
    let out = idea_tool
        .execute(json!({"action": "add", "title": "想法"}), &ctx)
        .await;
    assert!(!out.is_error, "idea 裸 add 不应被拦: {}", out.content);
    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
async fn tag_validation_skips_documents_without_vocabulary() {
    let dir = std::env::temp_dir().join(format!(
        "kz-tag-idea-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let store = DocStore::open(&dir, &IDEAS);
    store.save(&[entry("I-001")]).unwrap();
    let tool = TrackerTool {
        tool_name: "idea",
        noun: "idea",
        kind: &IDEAS,
        requires_refs: None,
    };
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    // 无词表的文档:任意标签值放行,不受校验约束。
    let out = tool
        .execute(
            json!({"action": "update", "id": "I-001", "fields": {"标签": "任意值"}}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "{}", out.content);
    std::fs::remove_dir_all(dir).ok();
}

/// R-138 验收③,打在**真实写入口**上:8 个并发的 `req add` 必须落 8 条、
/// ID 互不相同。这是 D-064 一族 lost-update 的直接回归锁——`kz` CLI、桌面端
/// 与自举循环是三个各自独立的 OS 进程,谁也看不见谁的内存态协调器,
/// 唯一挡得住的就是 execute 顶部那把跨进程锁。
#[test]
fn 并发新建不丢条目也不撞编号() {
    let dir = std::env::temp_dir().join(format!(
        "kz-tracker-concurrent-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    DocStore::open(&dir, &REQUIREMENTS).save(&[]).unwrap();

    let 并发 = 8usize;
    let handles: Vec<_> = (0..并发)
        .map(|n| {
            let dir = dir.clone();
            // 每个线程一个独立 runtime:模拟各自跑一个进程,不共享任何内存态。
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                rt.block_on(async move {
                    let tool = TrackerTool {
                        tool_name: "req",
                        noun: "requirement",
                        kind: &REQUIREMENTS,
                        requires_refs: None,
                    };
                    let ctx = ToolCtx::new(dir.clone(), dir.clone());
                    tool.execute(
                        json!({"action": "add", "title": format!("并发条目 {n}"), "priority": "P2", "fields": {"复杂度": "中", "标签": "后端", "来源": "用户原话「测试」", "发现记录": "{\"Intent\":\"测试意图\",\"Explicit\":\"用户原话\",\"Assumptions\":\"无\",\"Ambiguities\":\"无\",\"领域对象\":\"条目\",\"最小成功闭环\":\"登记\",\"延后决策\":\"无\"}"}}),
                        &ctx,
                    )
                    .await
                })
            })
        })
        .collect();
    for handle in handles {
        let out = handle.join().unwrap();
        assert!(!out.is_error, "并发 add 不该失败: {}", out.content);
    }

    let store = DocStore::open(&dir, &REQUIREMENTS);
    let entries = store.load().unwrap();
    assert_eq!(entries.len(), 并发, "有条目被覆盖掉了: {entries:?}");
    let ids: std::collections::BTreeSet<&String> = entries.iter().map(|e| &e.id).collect();
    assert_eq!(ids.len(), 并发, "分配出了重复 ID: {ids:?}");
    assert!(
        store.integrity_issues(&entries).unwrap().is_empty(),
        "并发写之后完整性必须干净: {:?}",
        store.integrity_issues(&entries).unwrap()
    );
    std::fs::remove_dir_all(dir).ok();
}
