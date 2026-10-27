/// D-737:direct update 到终态必须共享 close 门禁，不能绕过 observed_head 校验。
#[tokio::test]
async fn update直达终态复用关闭门禁() {
    let dir = std::env::temp_dir().join(format!("kz-update-terminal-gate-{}", std::process::id()));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let mut e = entry("R-001");
    e.status = "doing".into();
    e.fields = vec![(
        "observed_head".into(),
        "0000000000000000000000000000000000000000".into(),
    )];
    let store = DocStore::open(&dir, &REQUIREMENTS);
    store.save(&[e]).unwrap();
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };

    let rejected = tool
        .execute(
            json!({"action": "update", "id": "R-001", "status": "done"}),
            &ctx,
        )
        .await;
    assert!(
        rejected.is_error,
        "direct update 不得绕过关闭门禁: {}",
        rejected.content
    );
    assert!(
        rejected.content.contains("不在当前 HEAD 祖先链"),
        "{}",
        rejected.content
    );
    assert_eq!(store.load().unwrap()[0].status, "doing");

    let mut repaired = store.load().unwrap();
    repaired[0].fields.retain(|(key, _)| key != "observed_head");
    store.save(&repaired).unwrap();
    let accepted = tool
        .execute(
            json!({"action": "update", "id": "R-001", "status": "done"}),
            &ctx,
        )
        .await;
    assert!(
        !accepted.is_error,
        "direct update 通过关闭门禁后应可迁移: {}",
        accepted.content
    );
    assert_eq!(store.load().unwrap()[0].status, "done");
    let telemetry = crate::close_telemetry::read_records(&dir);
    assert_eq!(
        telemetry.len(),
        1,
        "direct update 到终态也应进入关闭 telemetry"
    );
    assert_eq!(telemetry[0].entry_id, "R-001");
    std::fs::remove_dir_all(dir).ok();
}

/// R-356 B3:不变式字段已不再触发 close 门禁，历史字段作为普通自由文本处理。
#[tokio::test]
async fn close不再执行不变式字段门禁() {
    let dir = std::env::temp_dir().join(format!("kz-close-no-invariant-{}", std::process::id()));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let mut e = entry("R-001");
    e.status = "doing".into();
    e.fields = vec![
        ("标签".into(), "核心".into()),
        ("批次".into(), "1/1".into()),
        (
            "不变式".into(),
            r#"[{"kind":"grep","path":"state.txt","pattern":"^done$"}]"#.into(),
        ),
    ];
    let store = DocStore::open(&dir, &REQUIREMENTS);
    store.save(&[e]).unwrap();
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };

    let accepted = tool
        .execute(json!({"action": "close", "id": "R-001"}), &ctx)
        .await;
    assert!(
        !accepted.is_error,
        "不变式字段不应再触发 close 门禁: {}",
        accepted.content
    );
    assert_eq!(store.load().unwrap()[0].status, "done");
    std::fs::remove_dir_all(dir).ok();
}

/// R-306 验收⑤:未合并观测 head 的活动条目不得关闭。
#[tokio::test]
async fn close拒绝未进入当前祖先链的observed_head() {
    let dir = std::env::temp_dir().join(format!("kz-close-ancestor-{}", std::process::id()));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let mut e = entry("R-001");
    e.fields = vec![
        ("标签".into(), "核心".into()),
        ("批次".into(), "1/1".into()),
        (
            "observed_head".into(),
            "0000000000000000000000000000000000000000".into(),
        ),
    ];
    DocStore::open(&dir, &REQUIREMENTS).save(&[e]).unwrap();
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };
    let out = tool
        .execute(json!({"action": "close", "id": "R-001"}), &ctx)
        .await;
    assert!(out.is_error, "{}", out.content);
    assert!(
        out.content.contains("不在当前 HEAD 祖先链"),
        "{}",
        out.content
    );
    assert!(
        out.content.contains("拒绝关闭") && out.content.contains("收编"),
        "{}",
        out.content
    );
    std::fs::remove_dir_all(dir).ok();
}

/// R-232 验收③:close 幂等重入安全——已 done 条目再次 close 返回 no-op,
/// 不再跑关闭门禁(前端冒烟/批次/分类断言),且文件字节不变。
#[tokio::test]
async fn close幂等重入_已终态条目再次关闭返回noop() {
    let dir = std::env::temp_dir().join(format!("kz-idem-close-{}", std::process::id()));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let mut e = entry("R-001");
    e.status = "done".into();
    e.fields = vec![
        ("标签".into(), "前端".into()),
        ("批次".into(), "1/1".into()),
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
    let before_bytes = std::fs::read(&path).unwrap();

    // 已 done 的前端标签条目:无前端冒烟 passed 也不应被 R-228 拦(重入非新关闭)。
    let out = tool
        .execute(json!({"action": "close", "id": "R-001"}), &ctx)
        .await;
    assert!(!out.is_error, "{}", out.content);
    assert!(out.content.contains("no-op"), "{}", out.content);
    let after_bytes = std::fs::read(&path).unwrap();
    assert_eq!(before_bytes, after_bytes, "重入 close 必须零写入");
    let saved = DocStore::open(&dir, &REQUIREMENTS).load().unwrap();
    assert_eq!(saved[0].status, "done", "状态不得回退");

    // 重入带字段补写:不是纯 no-op,允许落盘(有真实变更)。
    let out = tool
        .execute(
            json!({"action": "close", "id": "R-001", "fields": {"进展": "2026-08-16 补记"}}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "{}", out.content);
    assert!(out.content.contains("进展"), "{}", out.content);
    let after_bytes = std::fs::read(&path).unwrap();
    assert_ne!(before_bytes, after_bytes, "补字段应落盘");
    std::fs::remove_dir_all(dir).ok();
}

/// 2026-08-16 审计门禁:验收条款对账——带圈条款号必须在关闭进展中逐条覆盖并带
/// 证据锚(T-/file:line/提交号),否则显式降级;沉默跳过即拒。真伪由波次审计另查。
#[tokio::test]
async fn 验收条款对账_沉默降级拒关_带锚或显式降级放行() {
    let dir = std::env::temp_dir().join(format!("kz-acc-rec-{}", std::process::id()));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    let store = || DocStore::open(&dir, &REQUIREMENTS);
    let with_fields = |fields: Vec<(&str, &str)>| {
        let mut e = entry("R-001");
        e.fields = fields
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        e
    };

    // ② 在进展中完全未提及:拒关并点名 ②。
    let e = with_fields(vec![
        ("验收", "①编译成功有实测;②截图被模型消费"),
        ("进展", "关闭:①T-1786000000 实测通过"),
    ]);
    store().save(&[e]).unwrap();
    let out = tool
        .execute(json!({"action": "close", "id": "R-001"}), &ctx)
        .await;
    assert!(out.is_error, "{}", out.content);
    assert!(out.content.contains('②'), "{}", out.content);
    assert!(out.content.contains("未提及"), "{}", out.content);

    // 条款提及了但 400 字符邻域内没有任何证据锚:拒关。
    let e = with_fields(vec![
        ("验收", "①编译成功有实测"),
        ("进展", "关闭:①做完了,效果很好,大家都说好"),
    ]);
    store().save(&[e]).unwrap();
    let out = tool
        .execute(json!({"action": "close", "id": "R-001"}), &ctx)
        .await;
    assert!(out.is_error, "{}", out.content);
    assert!(out.content.contains("无证据锚"), "{}", out.content);

    // 逐条带锚(T- 记录 / file:line / 提交号)放行。
    let e = with_fields(vec![
        ("验收", "①编译成功有实测;②截图被模型消费;③发版"),
        (
            "进展",
            "关闭:①T-1786000000 实测;②见 crates/kanzei-tools/src/plot_tool.rs:185;\
                 ③随 build-9a06e05 发布",
        ),
    ]);
    store().save(&[e]).unwrap();
    let out = tool
        .execute(json!({"action": "close", "id": "R-001"}), &ctx)
        .await;
    assert!(!out.is_error, "{}", out.content);

    // 显式降级与「由用户」缓办是合法覆盖形态:放行。
    let e = with_fields(vec![
        ("验收", "①真机全链路实测;②锁屏恢复"),
        (
            "进展",
            "关闭:①验收降级: 真机实测改为 viewport 自检,真机部分待补;②由用户执行",
        ),
    ]);
    store().save(&[e]).unwrap();
    let out = tool
        .execute(json!({"action": "close", "id": "R-001"}), &ctx)
        .await;
    assert!(!out.is_error, "{}", out.content);

    // 无编号条款的验收不受影响(门禁只认带圈数字)。
    let e = with_fields(vec![
        ("验收", "一句话验收,无编号条款"),
        ("进展", "关闭:做完了"),
    ]);
    store().save(&[e]).unwrap();
    let out = tool
        .execute(json!({"action": "close", "id": "R-001"}), &ctx)
        .await;
    assert!(!out.is_error, "{}", out.content);
    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
async fn 批次没走完不能关闭_改小总数或做完都放行() {
    let dir = std::env::temp_dir().join(format!("kz-batch-close-{}", std::process::id()));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let mut e = entry("R-001");
    e.status = "doing".into();
    e.fields = vec![
        ("复杂度".into(), "大".into()),
        ("批次".into(), "3/11".into()),
    ];
    DocStore::open(&dir, &REQUIREMENTS).save(&[e]).unwrap();
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };

    let out = tool
        .execute(json!({"action": "close", "id": "R-001"}), &ctx)
        .await;
    assert!(out.is_error, "还剩 8 格空着就该拦下来: {}", out.content);
    assert!(out.content.contains("3/11"), "{}", out.content);

    // 当初估多了:把总数改成实际批数即可关闭——比留着空格诚实。
    let out = tool
        .execute(
            json!({"action": "close", "id": "R-001", "fields": {"批次": "3/3"}}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "总数改成实际批数后应放行: {}", out.content);
    let saved = DocStore::open(&dir, &REQUIREMENTS).load().unwrap();
    assert_eq!(saved[0].status, "done");
    std::fs::remove_dir_all(dir).ok();
}

/// 项目里一条 `scripts/ui-*.mjs` 都没有时,「必须跑过 ui smoke」是一条
/// 无法执行的指令,不是没跑。这道门只在项目确实提供冒烟脚本时成立。
///
/// 反例形态(实测):同一套 harness 跑在别的项目上,前端标签条目关闭被拒,
/// agent 去 glob 一个不存在的脚本、记一条失败测试,条目仍 open——整段零价值。
#[tokio::test]
async fn 门禁能力条件式_项目无冒烟脚本时前端标签照常可关闭() {
    let dir = std::env::temp_dir().join(format!(
        "kz-frontend-close-nogate-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let mut front = entry("R-001");
    front.status = "doing".into();
    front.fields = vec![("标签".into(), "前端".into())];
    DocStore::open(&dir, &REQUIREMENTS).save(&[front]).unwrap();
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };
    let out = tool
        .execute(json!({"action": "close", "id": "R-001"}), &ctx)
        .await;
    assert!(
        !out.is_error,
        "项目不提供冒烟脚本时不该设这道门: {}",
        out.content
    );
    std::fs::remove_dir_all(dir).ok();
}

/// R-228 验收①③:带「前端」标签的条目关闭前必须已有前端冒烟 passed 测试记录
/// ——没有则拒绝(验收①);非前端标签条目不受影响(验收③)。
#[tokio::test]
async fn 前端标签关闭需前端冒烟passed_非前端不受影响() {
    let dir = std::env::temp_dir().join(format!("kz-frontend-close-{}", std::process::id()));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    // 门禁是能力条件式的——项目得真的提供冒烟脚本,这道门才成立。
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(dir.join("scripts/ui-runtime-smoke.mjs"), "// fixture\n").unwrap();
    let mut front = entry("R-001");
    front.status = "doing".into();
    front.fields = vec![("标签".into(), "前端".into())];
    let mut backend = entry("R-002");
    backend.status = "doing".into();
    backend.fields = vec![("标签".into(), "核心".into())];
    DocStore::open(&dir, &REQUIREMENTS)
        .save(&[front, backend])
        .unwrap();
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };

    // 无任何前端冒烟 passed:前端标签条目关闭被拒,非前端放行。
    let out = tool
        .execute(json!({"action": "close", "id": "R-001"}), &ctx)
        .await;
    assert!(
        out.is_error && out.content.contains("前端"),
        "前端标签条目无前端冒烟 passed 必须拒绝关闭: {}",
        out.content
    );
    let out = tool
        .execute(json!({"action": "close", "id": "R-002"}), &ctx)
        .await;
    assert!(
        !out.is_error,
        "非前端标签条目不受前端门禁影响: {}",
        out.content
    );

    // 补一条前端冒烟 passed:前端标签条目可关闭。
    let rec_dir = dir.clone();
    crate::test_record::append_test_run(
        &rec_dir,
        "node scripts/ui-runtime-smoke.mjs (R-228)",
        "passed",
        Some("node scripts/ui-runtime-smoke.mjs"),
        None,
        None,
    )
    .unwrap();
    let out = tool
        .execute(json!({"action": "close", "id": "R-001"}), &ctx)
        .await;
    assert!(
        !out.is_error,
        "有前端冒烟 passed 后前端标签条目应可关闭: {}",
        out.content
    );
    let saved = DocStore::open(&dir, &REQUIREMENTS).load().unwrap();
    assert_eq!(saved[0].status, "done", "R-001 应已关闭");
    std::fs::remove_dir_all(dir).ok();
}

/// Narrative repetition cannot multiply structured acceptance obligations.
#[tokio::test]
async fn 重复叙述数量不增加关闭证据义务() {
    let dir = std::env::temp_dir().join(format!("kz-close-count-{}", std::process::id()));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let mut e = entry("R-001");
    e.status = "doing".into();
    e.title = "剩余 2 处归类".into();
    e.fields = vec![(
        "进展".into(),
        "剩余 2 处归类完成：src/first.rs:10 src/second.rs:20".into(),
    )];
    let store = DocStore::open(&dir, &REQUIREMENTS);
    store.save(&[e]).unwrap();
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    let output = tool
        .execute(json!({"action":"close","id":"R-001"}), &ctx)
        .await;
    assert!(!output.is_error, "{}", output.content);
    assert_eq!(store.load().unwrap()[0].status, "done");
    std::fs::remove_dir_all(dir).unwrap();
}

/// R-229 验收③:无分类断言的关闭不受影响;「剩余价值」这类无数字用法不算断言。
#[tokio::test]
async fn 无分类断言关闭不受影响() {
    let dir = std::env::temp_dir().join(format!("kz-class-close-free-{}", std::process::id()));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let mut e = entry("R-001");
    e.status = "doing".into();
    // 无「剩余/其余 N 处」断言,仅普通叙述;含「剩余价值」非断言用法。
    e.fields = vec![("进展".into(),
        "实现已落地,验证通过。剩余价值是声明与检测入口。crates/kanzei-tools/src/tracker.rs:100 为既有实现。".into())];
    DocStore::open(&dir, &REQUIREMENTS).save(&[e]).unwrap();
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };

    let out = tool
        .execute(json!({"action": "close", "id": "R-001"}), &ctx)
        .await;
    assert!(!out.is_error, "无分类断言不受影响: {}", out.content);
    let saved = DocStore::open(&dir, &REQUIREMENTS).load().unwrap();
    assert_eq!(saved[0].status, "done");
    std::fs::remove_dir_all(dir).ok();
}

/// D-241 机制项:fixing 推不动时可以合法退回 open。要求 id + reason,
/// 理由必须落进条目进展(不能只出现在工具输出),调度器下次看到的是 open。
#[tokio::test]
async fn reopen_把fixing退回open_并强制写理由进进展() {
    let dir = std::env::temp_dir().join(format!("kz-reopen-{}", std::process::id()));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let mut e = entry("D-001");
    e.status = "fixing".into();
    e.fields = vec![("进展".into(), "修复方向已落地,真机复测卡在外部环境".into())];
    DocStore::open(&dir, &DEFECTS).save(&[e]).unwrap();
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    let tool = TrackerTool {
        tool_name: "defect",
        noun: "defect",
        kind: &DEFECTS,
        requires_refs: None,
    };

    // 不带理由 → 拒绝。
    let out = tool
        .execute(json!({"action": "reopen", "id": "D-001"}), &ctx)
        .await;
    assert!(out.is_error, "reopen 必须给理由: {}", out.content);
    assert!(out.content.contains("reason"), "{}", out.content);

    // 状态不在 reopen 集合(open 不是) → 拒绝。
    let saved = DocStore::open(&dir, &DEFECTS).load().unwrap();
    assert_eq!(saved[0].status, "fixing", "被拒的 reopen 不能动状态");
    let out = tool
        .execute(
            json!({"action": "reopen", "id": "D-001", "reason": "推不动,退回"}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "fixing 应可退回 open: {}", out.content);
    assert!(out.content.contains("open"), "{}", out.content);

    let saved = DocStore::open(&dir, &DEFECTS).load().unwrap();
    assert_eq!(saved[0].status, "open", "状态必须退回 open");
    let progress_lines: Vec<&str> = saved[0]
        .fields
        .iter()
        .filter(|(k, _)| k == "进展")
        .map(|(_, v)| v.as_str())
        .collect();
    assert!(
        progress_lines.iter().any(|p| p.contains("推不动,退回")),
        "理由必须落进进展(任意一行): {:?}",
        progress_lines
    );
    assert!(
        progress_lines.iter().any(|p| p.contains("[reopen")),
        "进展要有 reopen 落款: {:?}",
        progress_lines
    );
    // 追加语义:原始进展行原样保留,新理由独立成行(与 docstore 按行解析一致)。
    assert!(
        progress_lines.iter().any(|p| p.contains("修复方向已落地")),
        "原始进展不能被覆盖: {:?}",
        progress_lines
    );
    std::fs::remove_dir_all(dir).ok();
}

/// D-241 边界:closed(终态)条目不能 reopen,requirement 的 done 同理。
#[tokio::test]
async fn reopen_拒绝终态与不在集合的状态() {
    let dir = std::env::temp_dir().join(format!("kz-reopen-edge-{}", std::process::id()));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let mut e = entry("D-001");
    e.status = "fixed".into();
    DocStore::open(&dir, &DEFECTS).save(&[e]).unwrap();
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    let tool = TrackerTool {
        tool_name: "defect",
        noun: "defect",
        kind: &DEFECTS,
        requires_refs: None,
    };
    let out = tool
        .execute(
            json!({"action": "reopen", "id": "D-001", "reason": "不该退"}),
            &ctx,
        )
        .await;
    assert!(out.is_error, "终态不可 reopen: {}", out.content);
    assert!(
        out.content.contains("not in the reopen set"),
        "{}",
        out.content
    );

    // requirement 的 doing 不在 reopen 集合(REQUIREMENTS.reopen_from = ["doing"] 时
    // 才可退;这里验证 todo 态也被拒),终态 done 更不行。
    let mut r = entry("R-001");
    r.status = "done".into();
    DocStore::open(&dir, &REQUIREMENTS).save(&[r]).unwrap();
    let rtool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };
    let out = rtool
        .execute(
            json!({"action": "reopen", "id": "R-001", "reason": "不该退"}),
            &ctx,
        )
        .await;
    assert!(out.is_error, "done 不可 reopen: {}", out.content);
    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
async fn r315_acceptance_scope_blocks_small_medium_patch_and_reports_history() {
    let dir = std::env::temp_dir().join(format!(
        "kz-r315-{}-{}",
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

    // D-568 历史形态重放:小复杂度承载「全量 INDEX」必须在登记时拒绝。
    let out = tool
        .execute(
            json!({
                "action": "add",
                "title": "历史全库核对",
                "priority": "P2",
                "tag": "流程",
                "complexity": "小",
                "fields": {"验收": "③全量 INDEX 行与对应 M-*.md 做一次机械一致性核对并修复"}
            }),
            &ctx,
        )
        .await;
    assert!(out.is_error, "D-568 形态应被登记门禁拦截: {}", out.content);
    assert!(out.content.contains("条款过开放"), "{}", out.content);

    // 中复杂度也不能用开放式全库审计逃避拆分。
    let out = tool
        .execute(
            json!({
                "action": "add",
                "title": "中复杂度全库核对",
                "priority": "P2",
                "tag": "流程",
                "complexity": "中",
                "fields": {
                    "验收": "①所有记录逐一核对并修复",
                    "来源": "用户原话「所有记录逐一核对并修复」",
                    "发现记录": discovery_record()
                }
            }),
            &ctx,
        )
        .await;
    assert!(out.is_error, "中复杂度开放条款应被拒: {}", out.content);
    assert!(out.content.contains("条款过开放"), "{}", out.content);

    // 大复杂度是允许承载开放条款的边界,登记仍必须成功。
    let out = tool
        .execute(
            json!({
                "action": "add",
                "title": "大复杂度全库核对",
                "priority": "P2",
                "tag": "流程",
                "complexity": "大",
                "fields": {
                    "验收": "①全量记录核对并修复",
                    "来源": "用户原话「全量记录核对并修复」",
                    "发现记录": discovery_record()
                }
            }),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "大复杂度开放条款应放行: {}", out.content);

    // 修订路径同样受门禁约束,不是只有 add 才拦。
    let out = tool
        .execute(
            json!({
                "action": "close",
                "id": "R-001",
                "fields": {"进展": "①已完成，T-123 crates/kanzei-tools/src/tracker.rs:1"}
            }),
            &ctx,
        )
        .await;
    assert!(
        out.is_error && out.content.contains("覆盖清单"),
        "大复杂度开放条款不能只用单点锚关闭: {}",
        out.content
    );
    let out = tool
        .execute(
            json!({
                "action": "close",
                "id": "R-001",
                "fields": {"进展": "①覆盖清单: INDEX 1 项；合计 1 项，T-123 crates/kanzei-tools/src/tracker.rs:1"}
            }),
            &ctx,
        )
        .await;
    assert!(
        !out.is_error,
        "带覆盖清单与计数后应允许关闭: {}",
        out.content
    );

    // 修订路径同样受门禁约束,不是只有 add 才拦。
    let out = tool
        .execute(
            json!({
                "action": "add",
                "title": "可修订条目",
                "priority": "P2",
                "tag": "流程",
                "complexity": "小",
                "fields": {"验收": "①完成指定文件修改"}
            }),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "封闭条款条目应先登记: {}", out.content);
    let out = tool
        .execute(
            json!({
                "action": "update",
                "id": "R-002",
                "fields": {"验收": "①所有文件逐一核对并修复"}
            }),
            &ctx,
        )
        .await;
    assert!(
        out.is_error,
        "验收 patch 不得绕过复杂度门禁: {}",
        out.content
    );
    assert!(out.content.contains("条款过开放"), "{}", out.content);

    // B4 扫描器读取活动存量并只列不匹配项,大复杂度开放条款不误报。
    let store = DocStore::open(&dir, &REQUIREMENTS);
    store
        .save(&[
            Entry {
                id: "R-001".into(),
                title: "存量小条目".into(),
                status: "todo".into(),
                severity: None,
                fields: vec![
                    ("复杂度".into(), "小".into()),
                    ("验收".into(), "①全量 INDEX 核对".into()),
                ],
            },
            Entry {
                id: "R-002".into(),
                title: "存量大条目".into(),
                status: "todo".into(),
                severity: None,
                fields: vec![
                    ("复杂度".into(), "大".into()),
                    ("验收".into(), "①全量 INDEX 核对".into()),
                ],
            },
        ])
        .unwrap();
    let report = tool
        .execute(json!({"action": "audit_acceptance_scope"}), &ctx)
        .await;
    assert!(!report.is_error, "扫描器应成功: {}", report.content);
    assert!(
        report.content.contains("\"mismatch_count\": 1"),
        "{}",
        report.content
    );
    assert!(report.content.contains("R-001"), "{}", report.content);
    assert!(
        !report.content.contains("存量大条目"),
        "大复杂度不应列入不匹配: {}",
        report.content
    );
    std::fs::remove_dir_all(dir).ok();
}

/// UI2-0926 #13 现场:`批次: 0/5; B1 核心模型、快速…` 整条被拒,模型反复重试。
/// 现在归一成 `0/5`,说明进「批次计划」字段。
#[tokio::test]
async fn 批次带附注的写法归一_说明进批次计划() {
    let dir = std::env::temp_dir().join(format!(
        "kz-batch-note-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let mut e = entry("R-001");
    e.status = "doing".into();
    DocStore::open(&dir, &REQUIREMENTS).save(&[e]).unwrap();
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };
    let out = tool
        .execute(
            json!({"action": "update", "id": "R-001", "fields": {"批次": "批次: 0/5; B1 核心模型、快速笔记"}}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "{}", out.content);
    let saved = DocStore::open(&dir, &REQUIREMENTS).load().unwrap();
    let field = |key: &str| {
        saved[0]
            .fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    };
    assert_eq!(field("批次").as_deref(), Some("0/5"));
    assert_eq!(field("批次计划").as_deref(), Some("B1 核心模型、快速笔记"));
    // 复核:归一写出的「批次计划」是登记过的叙事字段,结构化读取不标灰、不计入未知字段。
    let structured = super::scheduling::structured_entry(&saved[0], &[], false);
    let plan = structured["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "批次计划")
        .expect("结构化读取应含批次计划");
    assert_eq!(plan["known"], true, "{plan}");
    assert_eq!(plan["category"], "narrative", "{plan}");
    std::fs::remove_dir_all(&dir).ok();
}

/// 写入侧门禁必须真的接上:docstore::check_declared_batches 有实现有单测,但没有
/// 调用方时「上限 10」只是提示词里的一句话(§1.25:没有消费者不算交付)。
/// 同时钉死作用域——只校验**本次传入**的字段值,归档里 11/11 的历史条目照常关得掉。
#[tokio::test]
async fn 声明批数超过十批_写入侧拒绝_但不牵连已有的历史批数() {
    let dir = std::env::temp_dir().join(format!(
        "kz-batch-cap-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let mut e = entry("R-001");
    e.status = "doing".into();
    e.fields = vec![("批次".into(), "3/11".into())];
    DocStore::open(&dir, &REQUIREMENTS).save(&[e]).unwrap();
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };

    // ① 历史 3/11 的正常逐批推进(只改已完成数)必须放行:这是存量条目唯一能往前
    //    走的动作,拦掉它等于逼 agent 先篡改总数才能动这条。
    let out = tool
        .execute(
            json!({"action": "update", "id": "R-001", "fields": {"批次": "4/11"}}),
            &ctx,
        )
        .await;
    assert!(
        !out.is_error,
        "历史 3/11 推进到 4/11 应放行: {}",
        out.content
    );
    let saved = DocStore::open(&dir, &REQUIREMENTS).load().unwrap();
    assert_eq!(
        saved[0]
            .fields
            .iter()
            .find(|(k, _)| k == "批次")
            .map(|(_, v)| v.as_str()),
        Some("4/11"),
        "推进后的批次要真的落盘"
    );

    // ② 把既有的 11 抬到 16 是货真价实的新声明:拒绝,且错误里要同时给出上限、
    //    既有基准与出路(拆后续条目)。
    let out = tool
        .execute(
            json!({"action": "update", "id": "R-001", "fields": {"批次": "3/16"}}),
            &ctx,
        )
        .await;
    assert!(out.is_error, "抬高总数到 16 应被拒: {}", out.content);
    assert!(out.content.contains("10"), "{}", out.content);
    assert!(out.content.contains("11"), "{}", out.content);
    assert!(out.content.contains("后续条目"), "{}", out.content);

    // 既有值没超上限时,抬到 12 照旧撞门(基准不是免死金牌)。
    let out = tool
        .execute(
            json!({"action": "add", "title": "另一条", "priority": "P2", "fields": {"复杂度": "中", "标签": "后端", "来源": "用户原话「测试」", "发现记录": "{\"Intent\":\"测试意图\",\"Explicit\":\"用户原话\",\"Assumptions\":\"无\",\"Ambiguities\":\"无\",\"领域对象\":\"条目\",\"最小成功闭环\":\"登记\",\"延后决策\":\"无\"}", "批次": "0/5"}}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "{}", out.content);
    let out = tool
        .execute(
            json!({"action": "update", "id": "R-002", "fields": {"批次": "0/12"}}),
            &ctx,
        )
        .await;
    assert!(out.is_error, "既有 5 批抬到 12 应被拒: {}", out.content);

    // ③ 新建没有既有值,按 <=10 严格约束(两条写入路径都要接上,不能只堵一半)。
    let out = tool
        .execute(
            json!({"action": "add", "title": "新条目", "priority": "P2", "fields": {"复杂度": "中", "标签": "核心", "批次": "0/11"}}),
            &ctx,
        )
        .await;
    assert!(out.is_error, "add 携 11 批应被拒: {}", out.content);

    // ④ 新建 0/10 是合法上界,照常放行。
    let out = tool
        .execute(
            json!({"action": "add", "title": "十批条目", "priority": "P2", "fields": {"复杂度": "中", "标签": "后端", "来源": "用户原话「测试」", "发现记录": "{\"Intent\":\"测试意图\",\"Explicit\":\"用户原话\",\"Assumptions\":\"无\",\"Ambiguities\":\"无\",\"领域对象\":\"条目\",\"最小成功闭环\":\"登记\",\"延后决策\":\"无\"}", "批次": "0/10"}}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "add 携 10 批应放行: {}", out.content);

    // ⑤ 作用域:文件里已有的 11 是历史真值,把它做完(按实际改小成 4/4)必须照常
    //    关得掉——若门禁错误地校验合并后的整条目,归档里 11/11、16/16 的条目会永久
    //    关不掉。
    let out = tool
        .execute(
            json!({"action": "close", "id": "R-001", "fields": {"批次": "4/4"}}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "历史 11 批条目收口应放行: {}", out.content);
    assert_eq!(
        DocStore::open(&dir, &REQUIREMENTS).load().unwrap()[0].status,
        "done"
    );
    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
async fn 关闭时拒绝手写批次与_git_提交真源不一致() {
    let dir = std::env::temp_dir().join(format!(
        "kz-batch-git-close-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.email", "test@example.com"],
        vec!["config", "user.name", "Kanzei Test"],
        vec![
            "commit",
            "--allow-empty",
            "-q",
            "-m",
            "R-001 B1: first batch",
        ],
    ] {
        assert!(Command::new("git")
            .args(args)
            .current_dir(&dir)
            .status()
            .unwrap()
            .success());
    }
    let mut e = entry("R-001");
    e.status = "doing".into();
    e.fields = vec![("批次".into(), "2/2".into())];
    DocStore::open(&dir, &REQUIREMENTS).save(&[e]).unwrap();
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };

    let out = tool
        .execute(json!({"action": "close", "id": "R-001"}), &ctx)
        .await;
    assert!(
        out.is_error,
        "Git 与字段不一致必须拒绝关闭: {}",
        out.content
    );
    assert!(
        out.content.contains("Git 提交历史标记数为 1"),
        "{}",
        out.content
    );
    std::fs::remove_dir_all(dir).ok();
}
