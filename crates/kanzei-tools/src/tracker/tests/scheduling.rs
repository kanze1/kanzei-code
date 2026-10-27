#[tokio::test]
async fn list_stably_postpones_blocked_entries_and_restores_order() {
    let dir = std::env::temp_dir().join(format!(
        "kz-scheduler-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let store = DocStore::open(&dir, &REQUIREMENTS);
    let mut blocked = entry("R-001");
    blocked.fields.push(("阻塞".into(), "等待用户确认".into()));
    let mut dependency_blocked = entry("R-003");
    dependency_blocked
        .fields
        .push(("依赖".into(), "R-002".into()));
    let mut stage_blocked = entry("R-004");
    stage_blocked.fields.push(("阶段".into(), "5 后".into()));
    store
        .save(&[blocked, entry("R-002"), dependency_blocked, stage_blocked])
        .unwrap();
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };
    let ctx = ToolCtx::new(dir.clone(), dir.clone());

    let out = tool
        .execute(
            json!({"action": "list", "reason": "deduplicate_registration"}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "{}", out.content);
    let listed: serde_json::Value = serde_json::from_str(&out.content).unwrap();
    let items = listed["entries"].as_array().unwrap();
    assert_eq!(items[0]["id"], "R-002");
    assert_eq!(items[1]["id"], "R-001");
    assert!(items[1]["block_reasons"][0]
        .as_str()
        .unwrap()
        .contains("阻塞字段"));
    assert_eq!(items[2]["id"], "R-003");
    assert!(items[2]["block_reasons"][0]
        .as_str()
        .unwrap()
        .contains("未完成依赖: R-002"));
    assert_eq!(items[3]["id"], "R-004");
    assert!(items[3]["block_reasons"][0]
        .as_str()
        .unwrap()
        .contains("阶段门槛"));

    // 解除两个阻塞后，原文档顺序自动恢复；没有持久化一份临时排序。
    let out = tool
        .execute(
            json!({"action": "update", "id": "R-001", "fields": {"阻塞": ""}}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "{}", out.content);
    let out = tool
        .execute(
            json!({"action": "update", "id": "R-002", "status": "done"}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "{}", out.content);
    let out = tool
        .execute(
            json!({"action": "list", "reason": "deduplicate_registration"}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "{}", out.content);
    let listed: serde_json::Value = serde_json::from_str(&out.content).unwrap();
    let items = listed["entries"].as_array().unwrap();
    assert_eq!(items[0]["id"], "R-001");
    assert_eq!(items[1]["id"], "R-002");
    assert_eq!(items[2]["id"], "R-003");
    assert_eq!(items[3]["id"], "R-004");
    assert!(items[3]["block_reasons"][0]
        .as_str()
        .unwrap()
        .contains("阶段门槛"));
    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
async fn list_banners_deadlock_when_nothing_is_executable() {
    let dir = std::env::temp_dir().join(format!(
        "kz-deadlock-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let store = DocStore::open(&dir, &REQUIREMENTS);
    let mut first = entry("R-001");
    first
        .fields
        .push(("阻塞".into(), "等待用户确认方案".into()));
    let mut second = entry("R-002");
    second.fields.push(("阻塞".into(), "依赖外部服务".into()));
    store.save(&[first, second]).unwrap();
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };
    let ctx = ToolCtx::new(dir.clone(), dir.clone());

    let out = tool
        .execute(
            json!({"action": "list", "reason": "deduplicate_registration"}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "{}", out.content);
    let listed: serde_json::Value = serde_json::from_str(&out.content).unwrap();
    assert_eq!(listed["deadlocked"], true);
    assert!(listed["deadlock_guidance"]
        .as_str()
        .unwrap()
        .starts_with("[调度死锁] 2 条requirement"));
    // 横幅必须堵死"没有可执行条目"这条收尾话术,否则鞭挞照旧静默停。
    assert!(
        listed["deadlock_guidance"]
            .as_str()
            .unwrap()
            .contains("禁止以「没有可执行条目」"),
        "{}",
        out.content
    );
    assert!(out.content.contains("R-001") && out.content.contains("R-002"));

    // 只要有一条恢复可执行,横幅就消失——它只在真死锁时出现,不构成日常噪音。
    let out = tool
        .execute(
            json!({"action": "update", "id": "R-001", "fields": {"阻塞": ""}}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "{}", out.content);
    let out = tool
        .execute(
            json!({"action": "list", "reason": "deduplicate_registration"}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "{}", out.content);
    let listed: serde_json::Value = serde_json::from_str(&out.content).unwrap();
    assert_eq!(listed["deadlocked"], false, "{}", out.content);
    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
async fn list_names_dependency_cycles_instead_of_endless_waiting() {
    let dir = std::env::temp_dir().join(format!(
        "kz-cycle-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let store = DocStore::open(&dir, &REQUIREMENTS);
    let mut a = entry("R-001");
    a.fields.push(("依赖".into(), "R-002".into()));
    let mut b = entry("R-002");
    b.fields.push(("依赖".into(), "R-001".into()));
    // 环外的普通未完成依赖仍应照旧报"未完成依赖",不被环检测吞掉。
    let mut c = entry("R-003");
    c.fields.push(("依赖".into(), "R-004".into()));
    store.save(&[a, b, c, entry("R-004")]).unwrap();
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };
    let ctx = ToolCtx::new(dir.clone(), dir.clone());

    let out = tool
        .execute(
            json!({"action": "list", "reason": "deduplicate_registration"}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "{}", out.content);
    let listed: serde_json::Value = serde_json::from_str(&out.content).unwrap();
    let items = listed["entries"].as_array().unwrap();
    let cycle_line = items
        .iter()
        .find(|item| item["id"] == "R-001")
        .and_then(|item| item["block_reasons"][0].as_str())
        .unwrap_or_default();
    assert!(
        cycle_line.contains("循环依赖: R-001 → R-002 → R-001"),
        "{}",
        out.content
    );
    assert!(cycle_line.contains("断掉其中一条边"), "{}", out.content);
    let plain = items
        .iter()
        .find(|item| item["id"] == "R-003")
        .and_then(|item| item["block_reasons"][0].as_str())
        .unwrap_or_default();
    assert!(plain.contains("未完成依赖: R-004"), "{}", out.content);
    assert!(!plain.contains("循环依赖"), "{}", out.content);

    // 断边后环消失,两条都回到可执行。
    let out = tool
        .execute(
            json!({"action": "update", "id": "R-002", "fields": {"依赖": ""}}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "{}", out.content);
    let out = tool
        .execute(
            json!({"action": "list", "reason": "deduplicate_registration"}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "{}", out.content);
    assert!(!out.content.contains("循环依赖"), "{}", out.content);
    std::fs::remove_dir_all(dir).ok();
}
