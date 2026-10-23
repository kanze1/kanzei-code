//! User planning goes through the same WorkTool transaction as agent planning.
use kanzei_harness::orchestration::{ProjectExecutionCoordinator, WriterLeaseRequest};
use serde_json::{json, Value};

#[tauri::command]
pub async fn work_node_decompose(
    state: tauri::State<'_, crate::AppState>,
    project_dir: String,
    id: String,
    expected_sequence: i64,
    reason: String,
    children: Value,
) -> Result<Value, String> {
    let root = crate::normalized_project_root(std::path::Path::new(&project_dir));
    if state.coordinator.snapshot(&root).writer.is_some() {
        return Err("项目正在执行，请等待当前执行结束后再提交裂变".into());
    }
    let _lease = state
        .coordinator
        .acquire_writer_lease(WriterLeaseRequest {
            write_scope: root.clone(),
            run_id: format!(
                "work_plan_{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|error| error.to_string())?
                    .as_nanos()
            ),
            process_id: "work-plan-editor".into(),
            reason: "用户提交节点裂变".into(),
        })
        .await
        .map_err(|error| error.to_string())?;
    apply_plan(
        &root,
        json!({"action":"decompose", "id":id,
        "expected_sequence":expected_sequence, "reason":reason, "children":children}),
    )
}

fn apply_plan(root: &std::path::Path, input: Value) -> Result<Value, String> {
    let ctx = kanzei_harness::ToolCtx::new(root.to_path_buf(), root.to_path_buf());
    let output = kanzei_tools::work::WorkTool.decompose_from_user(input, &ctx);
    if output.is_error {
        return Err(match output.code {
            Some(code) => format!("{code}: {}", output.content),
            None => output.content,
        });
    }
    serde_json::from_str(&output.content).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_harness::Tool;
    #[tokio::test]
    async fn user_split_shares_versions_and_never_steals_an_active_node() {
        let root = kanzei_base::test_support::TestDir::new("ui-work-plan-");
        let docs =
            kanzei_memory::docstore::DocStore::open(&root, &kanzei_memory::docstore::REQUIREMENTS);
        docs.save(&[kanzei_memory::docstore::Entry {
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
        let ctx = kanzei_harness::ToolCtx::new(root.to_path_buf(), root.to_path_buf());
        let output = kanzei_tools::work::WorkTool.execute(json!({"action":"create_unit","requirement_id":"R-001","objective":"goal","acceptance":["works"],"base_revision":"fixture"}), &ctx).await;
        assert!(!output.is_error, "{}", output.content);
        let unit: Value = serde_json::from_str(&output.content).unwrap();
        let plan = json!({"action":"decompose","id":unit["unit_id"],"expected_sequence":unit["source_sequence"],"reason":"resolve unknowns","children":[{"key":"research","objective":"research","acceptance":["report"],"node":{"kind":"research","covers":["works"]}}]});
        let receipt = apply_plan(&root, plan.clone()).unwrap();
        assert_eq!(receipt["children"][0]["unit_id"], "R-001/W2");
        assert!(apply_plan(&root, plan.clone()).is_err());
        let store =
            kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
        let active = store
            .append_work_fact(
                "R-001/W2",
                kanzei_core::WorkFact::Claimed { claimed_by: None },
            )
            .unwrap();
        let mut plan = plan;
        plan["id"] = json!(active.unit_id);
        plan["expected_sequence"] = json!(active.source_sequence);
        assert!(apply_plan(&root, plan)
            .unwrap_err()
            .contains("WORK_OWNER_MISMATCH"));
        assert_eq!(store.list_work_units(None).unwrap().len(), 2);
    }
}
