use super::TrackerTool;
use crate::docstore::{DocKind, DocStore, Entry, DEFECTS, REQUIREMENTS};
use kanzei_harness::{Tool, ToolCtx, ToolOutcome};
use serde_json::json;

fn fixture(
    kind: &'static DocKind,
    action: &str,
) -> (std::path::PathBuf, ToolCtx, TrackerTool, DocStore) {
    let root = std::env::temp_dir().join(format!(
        "kz-backlog-{}-{action}-{}-{}",
        kind.prefix,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(root.join("scripts")).unwrap();
    std::fs::write(root.join("scripts/verify.ps1"), "# fixture").unwrap();
    std::fs::write(root.join("scripts/ui-lint-smoke.mjs"), "// fixture").unwrap();
    let store = DocStore::open(&root, kind);
    store
        .save(&[Entry {
            id: format!("{}-001", kind.prefix),
            title: "旧工作".into(),
            status: kind.statuses[1].into(),
            severity: kind.severities.map(|_| "medium".into()),
            fields: vec![
                ("标签".into(), "前端".into()),
                ("复杂度".into(), "小".into()),
                ("批次".into(), "0/3".into()),
                ("验收".into(), "①尚未实现；②无测试证据".into()),
            ],
        }])
        .unwrap();
    let ctx = ToolCtx::new(root.clone(), root.clone());
    let tool = TrackerTool {
        tool_name: if kind.prefix == "R" { "req" } else { "defect" },
        noun: "entry",
        kind,
        requires_refs: None,
    };
    (root, ctx, tool, store)
}

#[tokio::test]
async fn user_completion_preserves_criteria_and_bypasses_only_agent_delivery_evidence() {
    for kind in [&REQUIREMENTS, &DEFECTS] {
        for action in ["close", "update"] {
            let (root, ctx, tool, store) = fixture(kind, action);
            let id = format!("{}-001", kind.prefix);
            let mut entries = store.load().unwrap();
            entries[0]
                .fields
                .push(("进展".into(), "APK 已交付，用户已完成设备验收".into()));
            for (key, value) in &mut entries[0].fields {
                if key == "复杂度" {
                    *value = "大".into();
                }
                if key == "验收" {
                    *value = "①全部功能在 Android 设备上可用；②全部附件可检索".into();
                }
            }
            store.save(&entries).unwrap();
            let input = json!({"action":action,"id":id,"status":kind.terminal[0]});
            let bytes = std::fs::read(&store.path).unwrap();
            let agent = tool.execute(input.clone(), &ctx).await;
            assert!(
                agent.is_error,
                "agent completion must still require evidence"
            );
            assert_eq!(bytes, std::fs::read(&store.path).unwrap());
            let mut forged = input.clone();
            forged["user_action"] = json!(true);
            assert!(tool.execute(forged, &ctx).await.is_error);
            assert_eq!(bytes, std::fs::read(&store.path).unwrap());
            let output = tool.execute_user_action(input.clone(), &ctx).await;
            assert!(!output.is_error, "{}", output.content);
            let after = store.load().unwrap();
            assert_eq!(after[0].status, kind.terminal[0]);
            for (key, value) in &entries[0].fields {
                assert!(after[0].fields.contains(&(key.clone(), value.clone())));
            }
            assert!(after[0]
                .fields
                .iter()
                .any(|(key, value)| key == "用户验收" && value.contains("用户手动确认")));
            let completed = std::fs::read(&store.path).unwrap();
            assert!(!tool.execute_user_action(input, &ctx).await.is_error);
            assert_eq!(completed, std::fs::read(&store.path).unwrap());
            assert!(crate::close_telemetry::read_records(&root).is_empty());
            assert_eq!(
                crate::close_telemetry::rolling_metrics(&root).missing_evidence_total,
                0
            );
            store.archive_terminal().unwrap();
            assert_eq!(store.load_archive().unwrap()[0].status, kind.terminal[0]);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}

#[tokio::test]
async fn paired_conversations_do_not_acquire_autonomous_delivery_gates_or_fake_user_acceptance() {
    let (root, mut ctx, tool, store) = fixture(&REQUIREMENTS, "paired");
    ctx.project_workflow = false;
    let before = store.load().unwrap();
    let output = tool
        .execute(json!({"action":"close","id":"R-001","status":"done"}), &ctx)
        .await;
    assert!(!output.is_error, "{}", output.content);
    let after = store.load().unwrap();
    assert_eq!(after[0].status, "done");
    for field in &before[0].fields {
        assert!(after[0].fields.contains(field));
    }
    assert!(!after[0].fields.iter().any(|(name, _)| name == "用户验收"));
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn user_completion_still_validates_close_targets_and_retirement_reasons() {
    for kind in [&REQUIREMENTS, &DEFECTS] {
        let (root, ctx, tool, store) = fixture(kind, "user-validation");
        let id = format!("{}-001", kind.prefix);
        let bytes = std::fs::read(&store.path).unwrap();
        let invalid = tool
            .execute_user_action(json!({"action":"close","id":id,"status":"invented"}), &ctx)
            .await;
        assert_eq!(invalid.code, Some("TRACKER_CLOSE_TARGET_INVALID"));
        let missing = tool
            .execute_user_action(
                json!({"action":"close","id":id,"status":kind.terminal[1]}),
                &ctx,
            )
            .await;
        assert_eq!(missing.code, Some("TRACKER_CLOSE_REASON_REQUIRED"));
        let forged = tool
            .execute(
                json!({"action":"update","id":id,"fields":{"用户验收":"pretend"}}),
                &ctx,
            )
            .await;
        assert_eq!(forged.code, Some("TRACKER_ENGINE_FIELD"));
        assert_eq!(bytes, std::fs::read(&store.path).unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[tokio::test]
async fn backlog_maintenance_lists_both_queues_without_relaxing_execution_guard() {
    for kind in [&REQUIREMENTS, &DEFECTS] {
        let (root, ctx, tool, _) = fixture(kind, "list");
        let rejected = tool.execute(json!({"action":"list"}), &ctx).await;
        assert_eq!(rejected.outcome, ToolOutcome::NeedsCorrection);
        assert_eq!(rejected.code, Some("TRACKER_LIST_PURPOSE_REQUIRED"));
        let output = tool
            .execute(
                json!({"action":"list","reason":"backlog_maintenance"}),
                &ctx,
            )
            .await;
        assert!(!output.is_error, "{}", output.content);
        assert!(output.content.contains(&format!("{}-001", kind.prefix)));
        assert!(tool.description().contains("backlog_maintenance"));
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[tokio::test]
async fn retirement_requires_reason_bypasses_delivery_gates_and_preserves_unfinished_work() {
    for kind in [&REQUIREMENTS, &DEFECTS] {
        for action in ["close", "update"] {
            let (root, ctx, tool, store) = fixture(kind, action);
            let id = format!("{}-001", kind.prefix);
            let before = std::fs::read(&store.path).unwrap();
            let missing = tool
                .execute(
                    json!({"action":action,"id":id,"status":kind.terminal[1]}),
                    &ctx,
                )
                .await;
            assert_eq!(missing.code, Some("TRACKER_CLOSE_REASON_REQUIRED"));
            assert_eq!(missing.outcome, ToolOutcome::NeedsCorrection);
            assert_eq!(before, std::fs::read(&store.path).unwrap());
            let blank = tool
                .execute(
                    json!({"action":action,"id":id,"status":kind.terminal[1],"reason":"  "}),
                    &ctx,
                )
                .await;
            assert!(blank.is_error);
            let output = tool.execute(json!({"action":action,"id":id,"status":kind.terminal[1],"reason":"已由新方案替代，原验收不再适用"}), &ctx).await;
            assert!(!output.is_error, "{}", output.content);
            let move_output = tool.execute(json!({"action":"archive"}), &ctx).await;
            assert!(!move_output.is_error, "{}", move_output.content);
            assert!(store.load().unwrap().is_empty());
            let archived = store.load_archive().unwrap();
            assert_eq!(archived[0].status, kind.terminal[1]);
            let metrics = crate::close_telemetry::rolling_metrics(&root);
            assert_eq!(metrics.retired_entries, 1);
            assert_eq!(metrics.closed_entries, 0);
            assert_eq!(metrics.missing_evidence_total, 0);
            assert!(archived[0]
                .fields
                .iter()
                .any(|(key, value)| key == "关闭原因" && value.contains("新方案")));
            assert!(archived[0]
                .fields
                .iter()
                .any(|(key, value)| key == "验收" && value.contains("尚未实现")));
            assert!(archived[0]
                .fields
                .iter()
                .any(|(key, value)| key == "批次" && value == "0/3"));
            let bytes = std::fs::read(store.archive_file()).unwrap();
            let replay = tool
                .execute(
                    json!({"action":"close","id":id,"status":kind.terminal[1],"reason":"重复关闭"}),
                    &ctx,
                )
                .await;
            assert_eq!(replay.code, Some("ALREADY_TERMINAL"));
            assert_eq!(bytes, std::fs::read(store.archive_file()).unwrap());
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}

#[tokio::test]
async fn completed_delivery_still_requires_acceptance_evidence() {
    for kind in [&REQUIREMENTS, &DEFECTS] {
        let (root, ctx, tool, store) = fixture(kind, "delivery");
        let before = std::fs::read(&store.path).unwrap();
        let output = tool.execute(json!({"action":"close","id":format!("{}-001",kind.prefix),"status":kind.terminal[0],"reason":"想清掉旧条目"}), &ctx).await;
        assert!(output.is_error, "{}", output.content);
        assert_eq!(before, std::fs::read(&store.path).unwrap());
        assert!(store.load_archive().unwrap().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[tokio::test]
async fn external_acceptance_preserves_unverified_checks_and_does_not_block_development() {
    for kind in [&REQUIREMENTS, &DEFECTS] {
        let (root, ctx, tool, store) = fixture(kind, "external");
        let id = format!("{}-001", kind.prefix);
        let mut legacy = store.load().unwrap();
        legacy[0].fields.extend([
            ("状态".into(), kind.statuses[1].into()),
            ("Status".into(), kind.statuses[1].into()),
        ]);
        store.save(&legacy).unwrap();
        let invalid = tool
            .execute(
                json!({"action":"update","id":id,"status":"awaiting_external"}),
                &ctx,
            )
            .await;
        assert_eq!(invalid.code, Some("EXTERNAL_ACCEPTANCE_DETAIL_REQUIRED"));
        let output = tool.execute(json!({"action":"update","id":id,"status":"awaiting_external",
            "fields":{"进展":"实现与本地回归完成，证据 src/example.rs:12；SSH 现场未验证", "外部验收":"真实 SSH 完整运行、断线与取消"}}), &ctx).await;
        assert!(!output.is_error, "{}", output.content);
        let waiting = store.load().unwrap();
        assert_eq!(waiting[0].status, "awaiting_external");
        assert!(waiting[0]
            .fields
            .iter()
            .filter(|(key, _)| key == "状态" || key.eq_ignore_ascii_case("status"))
            .all(|(_, value)| value == "awaiting_external"));
        assert!(waiting[0]
            .fields
            .iter()
            .any(|(key, value)| key == "验收" && value.contains("尚未实现")));
        assert!(
            store.load_archive().unwrap().is_empty(),
            "外部验收未通过不能冒充完成或归档"
        );
        assert!(super::scheduling::workable_titles(&root, 10).is_empty());
        assert_eq!(
            super::scheduling::backlog_status(&root),
            kanzei_harness::auto_run::BacklogStatus::Empty
        );
        let control = crate::work::resolve_work_decision(
            &root,
            &root,
            kanzei_harness::auto_run::WorkPriority::DefectFirst,
        )
        .unwrap();
        assert_eq!(control.decision, crate::work::WorkDecision::Empty);
        assert!(control.blocked_items.is_empty() && control.executable_wip.is_empty());
        assert_eq!(control.pending_external.len(), 1);
        let mut dependent = waiting[0].clone();
        dependent.id = format!("{}-002", kind.prefix);
        dependent.status = kind.statuses[0].into();
        dependent.fields = vec![("依赖".into(), id.clone())];
        store.save(&[waiting[0].clone(), dependent]).unwrap();
        let control = crate::work::resolve_work_decision(
            &root,
            &root,
            kanzei_harness::auto_run::WorkPriority::DefectFirst,
        )
        .unwrap();
        assert_eq!(
            control.selected.as_ref().unwrap().id,
            format!("{}-002", kind.prefix)
        );
        assert!(control.blocked_items.is_empty());
        let resumed = tool
            .execute(
                json!({"action":"update","id":id,"status":kind.statuses[1]}),
                &ctx,
            )
            .await;
        assert!(!resumed.is_error, "{}", resumed.content);
        let resumed = store.load().unwrap();
        assert_eq!(resumed[0].status, kind.statuses[1]);
        assert!(resumed[0]
            .fields
            .iter()
            .filter(|(key, _)| key == "状态" || key.eq_ignore_ascii_case("status"))
            .all(|(_, value)| value == kind.statuses[1]));
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[tokio::test]
async fn reopen_external_acceptance_preserves_reason_and_status_mirrors() {
    for kind in [&REQUIREMENTS, &DEFECTS] {
        let (root, ctx, tool, store) = fixture(kind, "reopen-mirrors");
        let id = format!("{}-001", kind.prefix);
        let mut entries = store.load().unwrap();
        entries[0].status = "awaiting_external".into();
        entries[0].fields.extend([
            ("状态".into(), "awaiting_external".into()),
            ("Status".into(), "awaiting_external".into()),
            (
                "进展".into(),
                "local checks passed; external checks pending".into(),
            ),
            ("外部验收".into(), "SSH disconnect check".into()),
        ]);
        store.save(&entries).unwrap();
        let output = tool
            .execute(
                json!({"action":"reopen","id":id,"reason":"external check found a defect"}),
                &ctx,
            )
            .await;
        assert!(!output.is_error, "{}", output.content);
        let entries = store.load().unwrap();
        assert_eq!(entries[0].status, kind.statuses[0]);
        assert!(store.integrity_issues(&entries).unwrap().is_empty());
        assert!(entries[0]
            .fields
            .iter()
            .any(|(k, v)| k == "进展" && v.contains("local checks passed")));
        assert!(entries[0]
            .fields
            .iter()
            .any(|(k, v)| k == "进展" && v.contains("external check found a defect")));
        let next = tool.execute(json!({"action":"update","id":id,"fields":{"说明":"ready for another implementation pass"}}), &ctx).await;
        assert!(!next.is_error, "{}", next.content);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[tokio::test]
async fn claim_and_release_keep_legacy_status_mirrors_consistent() {
    let (root, ctx, _tool, store) = fixture(&DEFECTS, "claim-mirrors");
    let mut entries = store.load().unwrap();
    entries[0].status = "open".into();
    entries[0].fields.extend([
        ("状态".into(), "open".into()),
        ("Status".into(), "open".into()),
    ]);
    store.save(&entries).unwrap();
    let output = crate::work::WorkTool
        .execute(json!({"action":"claim","id":"D-001"}), &ctx)
        .await;
    assert!(!output.is_error, "{}", output.content);
    let mut entries = store.load().unwrap();
    assert_eq!(entries[0].status, "fixing");
    assert!(store.integrity_issues(&entries).unwrap().is_empty());
    entries[0]
        .fields
        .push(("取得线".into(), "fixture-line".into()));
    store.save(&entries).unwrap();
    assert_eq!(
        crate::work::release_line_claims(&root, "fixture-line", "external test follow-up").unwrap(),
        vec!["D-001"]
    );
    let entries = store.load().unwrap();
    assert_eq!(entries[0].status, "open");
    assert!(store.integrity_issues(&entries).unwrap().is_empty());
    assert!(entries[0]
        .fields
        .iter()
        .any(|(k, v)| k == "取活释放" && v.contains("external test follow-up")));
    std::fs::remove_dir_all(root).unwrap();
}
