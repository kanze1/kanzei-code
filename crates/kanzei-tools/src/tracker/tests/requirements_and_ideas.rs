/// R-252 验收③:想法转 split 的 refs 硬门禁——refs 空拒、指向不存在的 ID 拒、
/// 指向 requirements/defects 活跃或归档条目放行。走真实 tool.execute 链路。
#[tokio::test]
async fn idea_split_refs_gate_positive_and_negative() {
    let dir = std::env::temp_dir().join(format!(
        "kz-idea-split-gate-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let project = dir.join(".kanzei/project");
    std::fs::create_dir_all(&project).unwrap();
    // 想法线:一条 inbox 待拆解。
    std::fs::write(
        project.join("ideas.md"),
        "# Ideas\n\n## I-001 待拆解的想法 [inbox]\n",
    )
    .unwrap();
    // requirements:活跃 R-001 + 归档 R-002(defects 同构,归档放行依赖它)。
    std::fs::write(
        project.join("requirements.md"),
        "# Requirements\n\n## R-001 活跃需求 [todo]\n",
    )
    .unwrap();
    std::fs::write(
        project.join("requirements-archive.md"),
        "# Requirements Archive\n\n## R-002 已归档需求 [done]\n",
    )
    .unwrap();
    std::fs::write(
        project.join("defects.md"),
        "# Defects\n\n## D-001 活跃缺陷 [open]\n",
    )
    .unwrap();
    std::fs::write(
        project.join("defects-archive.md"),
        "# Defects Archive\n\n## D-002 已归档缺陷 [fixed]\n",
    )
    .unwrap();
    let tool = TrackerTool {
        tool_name: "idea",
        noun: "idea",
        kind: &IDEAS,
        requires_refs: None,
    };
    let ctx = ToolCtx::new(dir.clone(), dir.clone());

    // 反面 1:refs 为空 → 拒转 split。
    let out = tool
        .execute(
            json!({"action": "update", "id": "I-001", "status": "split", "refs": []}),
            &ctx,
        )
        .await;
    assert!(out.is_error, "refs 空必须拒: {}", out.content);
    assert!(out.content.contains("refs"), "{}", out.content);
    // 反面 2:refs 指向不存在的 ID → 拒。
    let out = tool
        .execute(
            json!({"action": "update", "id": "I-001", "status": "split", "refs": ["R-999"]}),
            &ctx,
        )
        .await;
    assert!(out.is_error, "指向不存在的 ID 必须拒: {}", out.content);
    assert!(out.content.contains("R-999"), "{}", out.content);
    // 反面 3:refs 指向非 R/D 编号 → 拒。
    let out = tool
        .execute(
            json!({"action": "update", "id": "I-001", "status": "split", "refs": ["S-001"]}),
            &ctx,
        )
        .await;
    assert!(out.is_error, "非 R/D 编号必须拒: {}", out.content);
    // 正面 1:refs 指向活跃 R-001 + D-001 → 放行,状态转 split。
    let out = tool
        .execute(
            json!({"action": "update", "id": "I-001", "status": "split", "refs": ["R-001", "D-001"]}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "指向活跃条目应放行: {}", out.content);
    let store = DocStore::open(&dir, &IDEAS);
    let entries = store.load().unwrap();
    assert_eq!(entries[0].status, "split");
    assert_eq!(
        entries[0].refs(),
        vec!["R-001", "D-001"],
        "转 split 的 refs 必须落到条目字段"
    );
    std::fs::remove_dir_all(dir).ok();
}

/// R-252 验收③:refs 指向归档条目放行——拆解产出的 R/D 可能随后完成并归档,
/// 不能因时序拒绝合法拆解。独立夹具,不受上一条测试写入影响。
#[tokio::test]
async fn idea_split_refs_gate_allows_archived_targets() {
    let dir = std::env::temp_dir().join(format!(
        "kz-idea-split-gate-arch-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let project = dir.join(".kanzei/project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("ideas.md"),
        "# Ideas\n\n## I-001 待拆解的想法 [inbox]\n",
    )
    .unwrap();
    // requirements 活动区为空,只有归档里的 R-002。
    std::fs::write(project.join("requirements.md"), "# Requirements\n").unwrap();
    std::fs::write(
        project.join("requirements-archive.md"),
        "# Requirements Archive\n\n## R-002 已归档需求 [done]\n",
    )
    .unwrap();
    let tool = TrackerTool {
        tool_name: "idea",
        noun: "idea",
        kind: &IDEAS,
        requires_refs: None,
    };
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    let out = tool
        .execute(
            json!({"action": "update", "id": "I-001", "status": "split", "refs": ["R-002"]}),
            &ctx,
        )
        .await;
    assert!(!out.is_error, "指向归档条目应放行: {}", out.content);
    let entries = DocStore::open(&dir, &IDEAS).load().unwrap();
    assert_eq!(entries[0].status, "split");
    std::fs::remove_dir_all(dir).ok();
}

/// R-252 验收③:非想法线不受 split 门禁约束——req 转 split 不是合法状态,
/// 但门禁逻辑只对 prefix==I 触发,req 的既有状态机校验照常兜底(不因门禁报
/// 错被误伤)。
#[tokio::test]
async fn idea_split_gate_skips_non_idea_lines() {
    let dir = std::env::temp_dir().join(format!(
        "kz-idea-split-gate-req-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let project = dir.join(".kanzei/project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("requirements.md"),
        "# Requirements\n\n## R-001 活跃需求 [todo]\n",
    )
    .unwrap();
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    // split 不是 req 的合法状态:应被状态机拒,而不是被 split 门禁拦(无 refs 也
    // 不该报 refs 相关错误——门禁对非 I 线完全跳过)。
    let out = tool
        .execute(
            json!({"action": "update", "id": "R-001", "status": "split"}),
            &ctx,
        )
        .await;
    assert!(out.is_error, "req 不能转 split: {}", out.content);
    assert!(
        !out.content.contains("refs"),
        "非想法线不应触发 refs 门禁: {}",
        out.content
    );
    std::fs::remove_dir_all(dir).ok();
}

/// R-313 B1/B3/B4:中/大登记需要结构化发现记录、用户原话与限定词处置；
/// 小需求保持既有登记路径。限定词可在明确标注 assumption 后放行。
#[tokio::test]
async fn r313_discovery_record_source_and_qualifier_gates() {
    let dir = std::env::temp_dir().join(format!(
        "kz-r313-discovery-{}-{}",
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

    let missing = tool
        .execute(
            json!({
                "action": "add",
                "title": "缺 Discovery Record",
                "priority": "P2",
                "fields": {"复杂度": "中", "标签": "后端"}
            }),
            &ctx,
        )
        .await;
    assert!(!missing.is_error, "{}", missing.content);
    assert!(missing.content.contains("[draft]"), "{}", missing.content);

    let no_quote = tool
        .execute(
            json!({
                "action": "add",
                "title": "缺用户原话",
                "priority": "P2",
                "fields": {
                    "复杂度": "中",
                    "标签": "后端",
                    "发现记录": discovery_record(),
                    "来源": "用户消息"
                }
            }),
            &ctx,
        )
        .await;
    assert!(!no_quote.is_error, "{}", no_quote.content);
    assert!(no_quote.content.contains("[draft]"), "{}", no_quote.content);

    let mismatch = tool
        .execute(
            json!({
                "action": "add",
                "title": "文章获取器收藏",
                "priority": "P1",
                "fields": {
                    "复杂度": "中",
                    "标签": "后端",
                    "发现记录": discovery_record(),
                    "来源": "用户原话「收藏」",
                    "限定词": "浏览器书签"
                }
            }),
            &ctx,
        )
        .await;
    assert!(
        mismatch.is_error,
        "未确认限定词必须拒绝: {}",
        mismatch.content
    );
    assert!(
        mismatch.content.contains("未确认解释"),
        "{}",
        mismatch.content
    );

    let released = tool
        .execute(
            json!({
                "action": "add",
                "title": "文章获取器收藏 assumption 放行",
                "priority": "P1",
                "fields": {
                    "复杂度": "中",
                    "标签": "后端",
                    "发现记录": discovery_record(),
                    "来源": "用户原话「收藏」",
                    "限定词": "浏览器书签",
                    "假设": "assumption: 浏览器书签是待确认的实现解释"
                }
            }),
            &ctx,
        )
        .await;
    assert!(
        !released.is_error,
        "标 assumption 后应放行: {}",
        released.content
    );

    let small = tool
        .execute(
            json!({
                "action": "add",
                "title": "既有小需求路径",
                "priority": "P3",
                "fields": {"复杂度": "小", "标签": "后端"}
            }),
            &ctx,
        )
        .await;
    assert!(!small.is_error, "小需求不得回归: {}", small.content);
    std::fs::remove_dir_all(dir).ok();
}

/// R-313 B2/B4:文章获取器的“收藏”歧义可登记为 todo，但进入 doing 前必须
/// 留下 question 作答凭证或用户豁免证据；两条放行路径都走真实 Tracker update。
/// D-764③收紧为凭证引用:散文式“已通过 question 确认”和模型自己的决定都不放行。
#[tokio::test]
async fn r313_pending_core_semantics_block_doing_and_allow_question_or_waiver() {
    let dir = std::env::temp_dir().join(format!(
        "kz-r313-lifecycle-{}-{}",
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
    let pending_fields = json!({
        "复杂度": "中",
        "标签": "后端",
        "来源": "用户原话「收藏」",
        "内容": "系统应保存用户选择的条目。",
            "发现记录": discovery_record(),
        "待确认": "核心语义未决：收藏是浏览器书签还是站内收藏"
    });
    let added = tool
        .execute(
            json!({
                "action": "add",
                "title": "文章获取器：收藏",
                "priority": "P1",
                "fields": pending_fields
            }),
            &ctx,
        )
        .await;
    assert!(!added.is_error, "发现阶段应先记录歧义: {}", added.content);

    let blocked = crate::work::WorkTool
        .execute(json!({"action": "claim", "id": "R-001"}), &ctx)
        .await;
    assert!(
        blocked.is_error,
        "未决核心语义进入 doing 必须被拦: {}",
        blocked.content
    );
    assert!(blocked.content.contains("question"), "{}", blocked.content);

    // D-764③:自述式留痕和模型自己的决定都不算用户确认——这正是过去被冒写的形态。
    for forged in [
        "已通过 question 确认：先支持知乎站内收藏",
        "question: 用户确认先支持知乎站内收藏",
        "Agent decision dec-1ec3ded4ec2dc3c9 已记录：先支持知乎站内收藏",
        "用户豁免",
        "用户明确豁免：模型自行判断即可",
        "用户明确豁免：原话「」",
        "无需用户豁免，也没有原话",
        "pending-question:run-9:call-1 尚未回答",
        "unanswered-question:run-9:call-1",
    ] {
        let refused = tool
            .execute(
                json!({
                    "action": "update",
                    "id": "R-001",
                    "status": "doing",
                    "fields": {"确认记录": forged}
                }),
                &ctx,
            )
            .await;
        assert!(refused.is_error, "冒写留痕必须被拦: {forged}");
    }

    let questioned = tool
        .execute(
            json!({
                "action": "update",
                "id": "R-001",
                "status": "doing",
                "fields": {"确认记录": "question:run-9:call-1 用户确认先支持知乎站内收藏"}
            }),
            &ctx,
        )
        .await;
    assert!(
        !questioned.is_error,
        "question 作答凭证应放行: {}",
        questioned.content
    );

    let waived = tool
        .execute(
            json!({
                "action": "add",
                "title": "文章获取器：收藏用户豁免",
                "priority": "P1",
                "fields": {
                    "复杂度": "中",
                    "标签": "后端",
                    "来源": "用户原话「收藏」",
                    "内容": "系统应保存用户选择的条目。",
            "发现记录": discovery_record(),
                    "待确认": "核心语义未决：收藏是否适配特定网站"
                }
            }),
            &ctx,
        )
        .await;
    assert!(
        !waived.is_error,
        "用户豁免场景应先记录歧义: {}",
        waived.content
    );
    let waived_start = tool
        .execute(
            json!({
                "action": "update",
                "id": "R-002",
                "status": "doing",
                "fields": {
                    "确认记录": "此前的提问尚未回答",
                    "用户豁免": "用户明确豁免：先只支持知乎站内收藏，原话为「先知乎就行」"
                }
            }),
            &ctx,
        )
        .await;
    assert!(
        !waived_start.is_error,
        "用户明确豁免应放行: {}",
        waived_start.content
    );
    std::fs::remove_dir_all(dir).ok();
}
