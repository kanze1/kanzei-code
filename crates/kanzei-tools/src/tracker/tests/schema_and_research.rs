#[test]
fn research_tracker_schema_only_exposes_get_and_add() {
    let tool = super::ResearchTrackerTool::new("req", "requirement", &REQUIREMENTS, None);
    let schema = tool.input_schema();
    let actions = schema
        .pointer("/properties/action/enum")
        .and_then(|value| value.as_array())
        .unwrap();
    assert_eq!(actions, &[json!("get"), json!("add")]);
}

#[tokio::test]
async fn topic_requirement_matches_schema_and_execution_for_all_research_record_actions() {
    let directory = kanzei_base::test_support::TestDir::new("kz-tracker-topic-schema-");
    let ctx = ToolCtx::new(directory.as_path().into(), directory.as_path().into());
    for (name, kind) in [("source", &SOURCES), ("finding", &FINDINGS)] {
        let tool = TrackerTool {
            tool_name: name,
            noun: name,
            kind,
            requires_refs: None,
        };
        let schema = tool.input_schema();
        assert!(schema["required"]
            .as_array()
            .unwrap()
            .contains(&json!("topic")));
        for action in ["list", "get", "add"] {
            let output = tool
                .execute(
                    json!({"action":action,"id":"S-001","title":"Unbound"}),
                    &ctx,
                )
                .await;
            assert!(output.is_error, "{name}/{action}: {}", output.content);
            assert!(output.content.contains("topic"), "{}", output.content);
        }
    }
    assert!(!directory.join(".kanzei").exists());
}

#[tokio::test]
async fn research_tracker_add_marks_todo_and_rejects_update() {
    let dir = std::env::temp_dir().join(format!(
        "kz-research-bridge-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".kanzei/project")).unwrap();
    let store = DocStore::open(&dir, &REQUIREMENTS);
    let tool = super::ResearchTrackerTool::new("req", "requirement", &REQUIREMENTS, None);
    let ctx = ToolCtx::new(dir.clone(), dir.clone());
    let added = tool
        .execute(
            json!({
                "action": "add",
                "title": "研究回流草稿",
                "priority": "P2",
                "fields": {"复杂度": "小", "标签": "流程"}
            }),
            &ctx,
        )
        .await;
    assert!(!added.is_error, "{}", added.content);
    let entry = store
        .load()
        .unwrap()
        .into_iter()
        .find(|entry| entry.title == "研究回流草稿")
        .expect("research add 应落一条需求草稿");
    assert_eq!(entry.status, "draft");
    assert!(entry
        .fields
        .iter()
        .any(|(key, value)| key == "回流" && value == "[todo]"));

    let rejected = tool
        .execute(
            json!({"action": "update", "id": entry.id, "fields": {"进展": "越权"}}),
            &ctx,
        )
        .await;
    assert!(rejected.is_error, "research wrapper 不得允许 update");
    let fetched = tool
        .execute(json!({"action": "get", "id": entry.id}), &ctx)
        .await;
    assert!(
        !fetched.is_error,
        "research get 应读取已登记条目: {}",
        fetched.content
    );
    assert!(fetched.content.contains("研究回流草稿"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn tracker_permission_resource_distinguishes_reads_and_writes() {
    let tool = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };
    assert_eq!(tool.resources(&json!({"action": "list"})), ["read:list"]);
    assert_eq!(tool.resources(&json!({"action": "get"})), ["read:get"]);
    assert_eq!(
        tool.resources(&json!({"action": "raw_lines"})),
        ["read:raw_lines"]
    );
    assert_eq!(tool.resources(&json!({"action": "add"})), ["write:add"]);
    assert_eq!(
        tool.resources(&json!({"action": "raw_delete"})),
        ["write:raw_delete"]
    );
    assert_eq!(
        tool.resources(&json!({"action": "repair_missing_id"})),
        ["write:repair_missing_id"]
    );
}

fn assert_update_requires_top_level_id(schema: &serde_json::Value) {
    let update_condition = schema["allOf"]
        .as_array()
        .unwrap()
        .iter()
        .find(|condition| condition["if"]["properties"]["action"]["const"] == "update")
        .expect("update action must have a conditional schema");
    assert_eq!(update_condition["then"]["required"], json!(["id"]));
    assert_eq!(
        update_condition["then"]["properties"]["id"],
        json!({"type": "string", "minLength": 1})
    );
}

/// priority 与 severity 是两个维度,合法取值必须进 schema 而不是只写在描述里。
#[test]
fn schema_gives_real_enums_for_each_document_kind() {
    use kanzei_memory::docstore::DEFECTS;
    let defects = TrackerTool {
        tool_name: "defect",
        noun: "defect",
        kind: &DEFECTS,
        requires_refs: None,
    };
    let schema = defects.input_schema();
    assert_update_requires_top_level_id(&schema);
    assert_eq!(
        schema["properties"]["severity"]["enum"],
        json!(["high", "medium", "low"])
    );
    assert_eq!(
        schema["properties"]["priority"]["enum"],
        json!(["P0", "P1", "P2", "P3"])
    );
    assert!(schema["properties"].get("complexity").is_none());
    let defect_required = schema["allOf"][0]["then"]["required"].as_array().unwrap();
    for field in ["title", "severity", "priority", "tag"] {
        assert!(defect_required.iter().any(|value| value == field));
    }
    assert_eq!(
        schema["properties"]["status"]["enum"],
        json!(["open", "fixing", "awaiting_external", "fixed", "wontfix"])
    );
    let actions = schema["properties"]["action"]["enum"].as_array().unwrap();
    for expected in ["list", "add", "void_id", "repair_missing_id"] {
        assert!(
            actions.iter().any(|a| a == expected),
            "action enum 缺 {expected}: {actions:?}"
        );
    }
    assert_eq!(
        schema["properties"]["severity"]["enum"],
        json!(["high", "medium", "low"])
    );
    assert_eq!(
        schema["properties"]["priority"]["enum"],
        json!(["P0", "P1", "P2", "P3"])
    );

    // 需求没有 severity 维度:schema 里根本不该出现这个字段。
    let requirements = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    };
    let schema = requirements.input_schema();
    assert_update_requires_top_level_id(&schema);
    assert!(schema["properties"].get("severity").is_none(), "{schema}");
    assert_eq!(
        schema["properties"]["priority"]["enum"],
        json!(["P0", "P1", "P2", "P3"])
    );
    assert_eq!(
        schema["properties"]["complexity"]["enum"],
        json!(["小", "中", "大"])
    );
    assert_eq!(
        schema["properties"]["tag"]["enum"],
        json!(["核心", "后端", "前端", "模型", "发布", "流程"])
    );
    let req_required = schema["allOf"][0]["then"]["required"].as_array().unwrap();
    assert_eq!(req_required, &vec![json!("title")]);
    assert!(schema["properties"].get("requirement").is_some());
    assert!(schema["properties"].get("evidence").is_some());
}

#[test]
fn prior_art字段只出现在requirement_schema() {
    let requirement = TrackerTool {
        tool_name: "req",
        noun: "requirement",
        kind: &REQUIREMENTS,
        requires_refs: None,
    }
    .input_schema();
    let defect = TrackerTool {
        tool_name: "defect",
        noun: "defect",
        kind: &DEFECTS,
        requires_refs: None,
    }
    .input_schema();
    assert!(requirement.pointer("/properties/prior_art").is_some());
    assert!(requirement
        .pointer("/properties/prior_art_waiver")
        .is_some());
    assert!(defect.pointer("/properties/prior_art").is_none());
    assert!(defect.pointer("/properties/prior_art_waiver").is_none());
}
