//! A start/continue button carries an exact item ID, not an instruction the model
//! must guess. Execute the existing claim gates only after acquiring the write slot.

use kanzei_core::{RunEvent, SessionStore};
use kanzei_harness::{Tool, ToolCtx};
use serde_json::json;

pub(crate) fn validate_requested_item(id: &str) -> Result<(), String> {
    let (outcome, node) = id
        .split_once("/W")
        .map_or((id, None), |(root, node)| (root, Some(node)));
    let digits = |value: &str| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit());
    let valid = outcome
        .strip_prefix("R-")
        .or_else(|| id.strip_prefix("D-"))
        .is_some_and(digits)
        && node.is_none_or(|number| outcome.starts_with("R-") && digits(number));
    if valid {
        Ok(())
    } else {
        Err("工作编号必须是 R-xxx、D-xxx 或 R-xxx/Wn".into())
    }
}

pub(crate) async fn claim_requested_work(
    ctx: &ToolCtx,
    item_id: &str,
    on_event: &mut (dyn FnMut(RunEvent) + Send),
) -> anyhow::Result<()> {
    validate_requested_item(item_id).map_err(anyhow::Error::msg)?;
    let input =
        json!({"action":"claim","id":item_id,"reason":"用户在工作台明确选择开始/继续此条目"});
    let call_id = format!("ui_claim_{}", ctx.run_id.as_deref().unwrap_or(""));
    on_event(RunEvent::ToolStart {
        id: call_id.clone(),
        name: "work".into(),
        summary: format!("claim {item_id}"),
        input: input.clone(),
    });
    let output = kanzei_tools::work::WorkTool.execute(input, ctx).await;
    on_event(RunEvent::tool_end(call_id, "work".into(), &output));
    anyhow::ensure!(!output.is_error, "无法开始 {item_id}: {}", output.content);
    let receipt: serde_json::Value = serde_json::from_str(&output.content)?;
    let store = SessionStore::open(&kanzei_core::project_state_path(&ctx.project_root))?;
    store.append_event(
        ctx.session_id.as_deref().unwrap_or(""),
        "work.ui_claimed",
        &json!({"run_id":ctx.run_id,"item_id":item_id,"receipt":receipt}),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_memory::docstore::{DocStore, REQUIREMENTS};

    #[tokio::test]
    async fn explicit_node_start_never_falls_back_to_another_node() {
        let root = kanzei_base::test_support::TestDir::new("ui-node-start-");
        DocStore::open(&root, &REQUIREMENTS)
            .save(&[kanzei_memory::docstore::Entry {
                id: "R-001".into(),
                title: "Goal".into(),
                status: "todo".into(),
                severity: None,
                fields: vec![
                    ("执行模型".into(), "work_units_v1".into()),
                    ("目标".into(), "goal".into()),
                ],
            }])
            .unwrap();
        let store = SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
        store
            .create_session("s", &root.display().to_string(), None)
            .unwrap();
        let ctx = ToolCtx::new(root.to_path_buf(), root.to_path_buf()).with_session_id("s".into());
        for n in 1..=2 {
            store
                .create_work_unit(kanzei_core::WorkUnitSpec {
                    unit_id: format!("R-001/W{n}"),
                    requirement_id: "R-001".into(),
                    objective: format!("work {n}"),
                    scope: vec![],
                    dependencies: vec![],
                    acceptance: vec!["works".into()],
                    verification: vec![],
                    base_revision: "fixture".into(),
                    node: Default::default(),
                })
                .unwrap();
        }
        store
            .append_work_fact(
                "R-001/W2",
                kanzei_core::WorkFact::Blocked {
                    reason: "external dependency".into(),
                },
            )
            .unwrap();
        assert!(claim_requested_work(&ctx, "R-001/W2", &mut |_| {})
            .await
            .is_err());
        assert_eq!(
            store.get_work_unit("R-001/W1").unwrap().unwrap().status,
            kanzei_core::WorkUnitStatus::Ready
        );
        store
            .append_work_fact(
                "R-001/W2",
                kanzei_core::WorkFact::Unblocked {
                    reason: "available".into(),
                },
            )
            .unwrap();
        claim_requested_work(&ctx, "R-001/W2", &mut |_| {})
            .await
            .unwrap();
        assert_eq!(
            store.get_work_unit("R-001/W2").unwrap().unwrap().status,
            kanzei_core::WorkUnitStatus::Active
        );
        assert_eq!(
            store.get_work_unit("R-001/W1").unwrap().unwrap().status,
            kanzei_core::WorkUnitStatus::Ready
        );
        for bad in ["R-001/W", "D-001/W2", "R-001/W2/W3", "R-001/W-1"] {
            assert!(validate_requested_item(bad).is_err());
        }
    }

    #[tokio::test]
    async fn start_claims_real_item_but_never_bypasses_discovery_gates() {
        let root = std::env::temp_dir().join(format!(
            "kz_ui_claim_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        std::fs::write(root.join(".kanzei/project/requirements.md"),
            "# Requirements\n\n## R-001 小修 [todo]\n- 复杂度: 小\n- 优先级: P1\n- 验收: 明确可验收\n\n## R-002 大需求 [todo]\n- 复杂度: 大\n- 优先级: P2\n").unwrap();
        let store = SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
        store
            .create_session("session", &root.display().to_string(), None)
            .unwrap();
        let ctx = ToolCtx::new(root.clone(), root.clone())
            .with_session_id("session".into())
            .with_identity(
                root.display().to_string(),
                root.display().to_string(),
                "run".into(),
                "process".into(),
            );
        let mut events = Vec::new();
        claim_requested_work(&ctx, "R-001", &mut |event| events.push(event))
            .await
            .unwrap();
        assert_eq!(
            DocStore::open(&root, &REQUIREMENTS).load().unwrap()[0].status,
            "doing"
        );
        assert_eq!(
            store
                .latest_work_claim("session", Some("run"))
                .unwrap()
                .as_deref(),
            Some("R-001")
        );
        assert_eq!(
            store
                .latest_work_claim("session", Some("other-run"))
                .unwrap(),
            None
        );
        assert!(matches!(
            events.last(),
            Some(RunEvent::ToolEnd { ok: true, .. })
        ));
        assert!(claim_requested_work(&ctx, "R-002", &mut |_| {})
            .await
            .is_err());
        assert_eq!(
            DocStore::open(&root, &REQUIREMENTS).load().unwrap()[1].status,
            "todo"
        );
        assert!(claim_requested_work(&ctx, "not-an-id", &mut |_| {})
            .await
            .is_err());
        drop(store);
        std::fs::remove_dir_all(root).ok();
    }
}
