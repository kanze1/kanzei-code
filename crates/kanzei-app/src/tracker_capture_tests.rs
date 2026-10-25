use super::*;
use kanzei_memory::docstore::{Entry, DEFECTS, IDEAS, REQUIREMENTS};
use serde_json::json;

fn registration(
    kind: &'static kanzei_memory::docstore::DocKind,
    source: &CaptureSession,
) -> RegistrationTracker {
    RegistrationTracker {
        inner: kanzei_tools::tracker::TrackerTool {
            tool_name: if kind.prefix == "R" { "req" } else { "defect" },
            noun: if kind.prefix == "R" {
                "requirement"
            } else {
                "defect"
            },
            kind,
            requires_refs: None,
        },
        source: source.clone(),
    }
}
fn requirement(title: &str) -> serde_json::Value {
    json!({"action":"add","title":title,"tag":"前端","requirement":{
        "statement":format!("用户请求时，系统应保存{title}。"),
        "acceptance":[{"text":format!("保存后能够读取{title}")}],
        "source":{"reference":"MODEL_INVENTED_SOURCE"}
    }})
}

#[tokio::test]
async fn registration_has_one_contract_without_legacy_conversion_or_existing_record_mutation() {
    let root = kanzei_base::test_support::TestDir::new("registration-contract");
    let original = "  用户只要求保存评分\n保留原文末尾空格  ";
    let source = save_source(&root, original).unwrap();
    let tool = registration(&REQUIREMENTS, &source);
    let ctx = ToolCtx::new(root.to_path_buf(), root.to_path_buf());
    for input in [
        json!({"action":"add","title":"legacy","fields":{"内容":"旧字段","验收":"旧验收"}}),
        json!({"action":"update","id":"R-001","requirement":{}}),
        json!({"action":"close","id":"R-001"}),
    ] {
        let output = tool.execute(input, &ctx).await;
        assert!(output.is_error, "{}", output.content);
    }
    assert!(source.ids().unwrap().is_empty());
    let store = DocStore::open(&root, &REQUIREMENTS);
    assert!(store.load().unwrap().is_empty());
    let output = tool.execute(requirement("评分"), &ctx).await;
    assert!(!output.is_error, "{}", output.content);
    assert_eq!(source.ids().unwrap(), ["R-001"]);
    let entry = &store.load().unwrap()[0];
    let spec = RequirementSpec::from_entry(entry).unwrap().unwrap();
    assert_eq!(spec.source.reference, source.reference);
    assert_eq!(
        std::fs::read_to_string(root.join(&spec.source.reference)).unwrap(),
        original
    );
    let before = std::fs::read(&store.path).unwrap();
    assert!(!tool.execute(requirement("评分"), &ctx).await.is_error);
    assert_eq!(std::fs::read(&store.path).unwrap(), before);
    assert_eq!(store.load().unwrap().len(), 1);
    assert_eq!(
        tool.input_schema()["properties"]["action"]["enum"],
        json!(["list", "get", "add", "reuse"])
    );
    assert!(tool.input_schema()["properties"].get("fields").is_none());
}

#[tokio::test]
async fn source_binding_and_reuse_receipts_cannot_be_fabricated_by_model_text() {
    let root = kanzei_base::test_support::TestDir::new("registration-receipts");
    let original = "用户在桌面端点击保存后出现错误";
    let source = save_source(&root, original).unwrap();
    let ctx = ToolCtx::new(root.to_path_buf(), root.to_path_buf());
    let tool = registration(&DEFECTS, &source);
    let output = tool.execute(json!({"action":"add","title":"桌面端保存错误","severity":"medium",
        "priority":"P2","tag":"前端","fields":{"复现":"点击保存后出现错误","原始描述":"FAKE_ORIGINAL","来源":"FAKE_SOURCE"}
    }), &ctx).await;
    assert!(!output.is_error, "{}", output.content);
    let store = DocStore::open(&root, &DEFECTS);
    let entry = &store.load().unwrap()[0];
    assert!(entry
        .fields
        .contains(&("来源".into(), source.reference.clone())));
    assert!(entry.fields.contains(&("原始描述".into(), original.into())));
    let another = save_source(&root, "复用之前登记的保存错误").unwrap();
    let reuse = registration(&DEFECTS, &another);
    assert!(
        reuse
            .execute(json!({"action":"reuse","id":"D-999"}), &ctx)
            .await
            .is_error
    );
    assert!(another.ids().unwrap().is_empty());
    let before = std::fs::read(&store.path).unwrap();
    assert!(
        !reuse
            .execute(json!({"action":"reuse","id":"D-001"}), &ctx)
            .await
            .is_error
    );
    assert_eq!(another.ids().unwrap(), ["D-001"]);
    assert_eq!(std::fs::read(&store.path).unwrap(), before);
}

#[tokio::test]
async fn idea_completion_is_explicit_scoped_and_requires_real_complete_receipts() {
    let root = kanzei_base::test_support::TestDir::new("idea-completion");
    let idea = Entry {
        id: "I-001".into(),
        title: "保存和导出评分".into(),
        status: "inbox".into(),
        severity: None,
        fields: vec![],
    };
    let ideas = DocStore::open(&root, &IDEAS);
    ideas.save(std::slice::from_ref(&idea)).unwrap();
    let source = save_source(
        &root,
        &kanzei_memory::docstore::render(&IDEAS, std::slice::from_ref(&idea)),
    )
    .unwrap();
    let ctx = ToolCtx::new(root.to_path_buf(), root.to_path_buf());
    let req = registration(&REQUIREMENTS, &source);
    for title in ["评分", "导出"] {
        let output = req.execute(requirement(title), &ctx).await;
        assert!(!output.is_error, "{}", output.content);
    }
    // Creating records is progress, not a completion declaration.
    assert_eq!(ideas.load().unwrap()[0].status, "inbox");
    let tool = IdeaSplitTracker {
        inner: kanzei_tools::tracker::TrackerTool {
            tool_name: "idea",
            noun: "idea",
            kind: &IDEAS,
            requires_refs: None,
        },
        idea: idea.clone(),
        source,
    };
    for input in [
        json!({"action":"update","id":"I-999","status":"split","refs":["R-001","R-002"]}),
        json!({"action":"update","id":"I-001","status":"split","title":"覆盖原文","refs":["R-001","R-002"]}),
        json!({"action":"update","id":"I-001","status":"split","refs":["R-001"]}),
        json!({"action":"update","id":"I-001","status":"split","refs":["R-001","R-002","R-999"]}),
        json!({"action":"update","id":"I-001","status":"split","refs":["R-001","R-002","R-002"]}),
    ] {
        let output = tool.execute(input, &ctx).await;
        assert!(output.is_error, "{}", output.content);
        assert_eq!(
            ideas.load().unwrap().as_slice(),
            std::slice::from_ref(&idea)
        );
    }
    let output = tool
        .execute(
            json!({"action":"update","id":"I-001","status":"split","refs":["R-001","R-002"]}),
            &ctx,
        )
        .await;
    assert!(!output.is_error, "{}", output.content);
    let saved = &ideas.load().unwrap()[0];
    assert_eq!(saved.title, idea.title);
    assert_eq!(saved.status, "split");
    assert!(saved
        .fields
        .contains(&("refs".into(), "R-001 R-002".into())));
}
