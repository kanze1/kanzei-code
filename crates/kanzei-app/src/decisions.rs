//! 决策复核应用服务：原会话返工队列 + 显式范围的用户偏好。
use kanzei_core::store::decisions::{
    DecisionRecord, DecisionStatus, PreferenceScope, ReviewAction, ReviewDecision,
    PREFERENCE_APPLIED_EVENT,
};
use kanzei_tools::memory::{AddOutcome, MemoryStore};
use std::path::Path;

#[tauri::command(async)]
pub(crate) fn verification_cancel(project_dir: String, job_id: String) -> Result<(), String> {
    kanzei_tools::verification::cancel(
        &crate::normalized_project_root(Path::new(&project_dir)),
        &job_id,
    )
    .map_err(|e| e.to_string())
}

/// 仅把已确认且已入库的偏好连到决策来源；每次叠加在文件图缓存副本上。
pub(crate) fn append_graph_lineage(
    root: &Path,
    graph: &mut serde_json::Value,
) -> anyhow::Result<()> {
    let state_path = kanzei_core::project_state_path(root);
    if !state_path.exists() {
        return Ok(());
    }
    let store = kanzei_core::SessionStore::open(&state_path)?;
    for decision in store.list_decisions(&kanzei_core::project_session_id(root))? {
        let Some(review) = &decision.review else {
            continue;
        };
        let Some(memory_id) = &review.preference_id else {
            continue;
        };
        let scope = match review.scope {
            PreferenceScope::Project => "project",
            PreferenceScope::Global => "global",
            PreferenceScope::Once => continue,
        };
        let nodes = graph["nodes"]
            .as_array_mut()
            .ok_or_else(|| anyhow::anyhow!("graph nodes unavailable"))?;
        let Some(memory) = nodes
            .iter_mut()
            .find(|n| n["id"] == *memory_id && n["scope"] == scope && n["status"] == "active")
        else {
            continue;
        };
        memory["degree"] = serde_json::json!(memory["degree"].as_u64().unwrap_or(0) + 1);
        nodes.push(serde_json::json!({
            "id": decision.id, "kind": "decision", "label": format!("DEC · {}", &decision.id[4..12]),
            "title": decision.question, "description": format!("用户纠正：{}", review.feedback),
            "scope": "project", "category": null, "status": "reviewed", "archived": false,
            "updated": review.reviewed_at.to_string(), "hits": 0, "areas": [], "primary_area": null,
            "area_provenance": null, "degree": 1,
        }));
        graph["edges"].as_array_mut().ok_or_else(|| anyhow::anyhow!("graph edges unavailable"))?.push(serde_json::json!({
            "source": memory_id, "target": decision.id, "rel": "derived_from", "strength": "strong",
            "provenance": null, "via": null, "anchor": null,
        }));
    }
    let journal = kanzei_core::project_session_id(root);
    let decisions = store.list_decisions(&journal)?;
    let applied = store.list_events_by_type(&journal, 0, PREFERENCE_APPLIED_EVENT)?;
    for event in applied {
        let Some(decision_id) = event.payload["decision_id"].as_str() else {
            continue;
        };
        let Some(preference_ref) = event.payload["preference_ref"].as_str() else {
            continue;
        };
        if event.payload["asserted_by"] != "agent" {
            continue;
        }
        let Some(decision) = decisions.iter().find(|decision| {
            decision.id == decision_id
                && decision.status == DecisionStatus::Decided
                && decision.resolution.as_ref().is_some_and(|resolution| {
                    resolution
                        .preference_refs
                        .iter()
                        .any(|r| r == preference_ref)
                })
        }) else {
            continue;
        };

        let memory_indices: Vec<usize> = graph["nodes"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("graph nodes unavailable"))?
            .iter()
            .enumerate()
            .filter(|(_, node)| {
                node["id"] == preference_ref
                    && node["status"] == "active"
                    && matches!(node["scope"].as_str(), Some("project") | Some("global"))
            })
            .map(|(index, _)| index)
            .collect();
        if memory_indices.len() != 1 {
            // ID 缺失、失效或 project/global 同 ID 时没有足够事实唯一定位，不能造边。
            continue;
        }
        let edges = graph["edges"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("graph edges unavailable"))?;
        if edges.iter().any(|edge| {
            edge["source"] == decision_id
                && edge["target"] == preference_ref
                && edge["rel"] == "refs"
        }) {
            continue;
        }

        let decision_node = graph["nodes"]
            .as_array()
            .expect("nodes checked above")
            .iter()
            .position(|node| node["id"] == decision_id && node["kind"] == "decision");
        let nodes = graph["nodes"].as_array_mut().expect("nodes checked above");
        let memory_index = memory_indices[0];
        let memory_degree = nodes[memory_index]["degree"].as_u64().unwrap_or(0) + 1;
        nodes[memory_index]["degree"] = serde_json::json!(memory_degree);
        if let Some(index) = decision_node {
            let degree = nodes[index]["degree"].as_u64().unwrap_or(0) + 1;
            nodes[index]["degree"] = serde_json::json!(degree);
        } else {
            nodes.push(serde_json::json!({
                "id": decision.id,
                "kind": "decision",
                "label": format!("DEC · {}", &decision.id[4..12]),
                "title": decision.question,
                "description": format!("明确采用偏好 {preference_ref}"),
                "scope": "project",
                "category": null,
                "status": "decided",
                "archived": false,
                "updated": decision.created_at.to_string(),
                "hits": 0,
                "areas": [],
                "primary_area": null,
                "area_provenance": null,
                "degree": 1,
            }));
        }
        graph["edges"]
            .as_array_mut()
            .expect("edges checked above")
            .push(serde_json::json!({
                "source": decision_id,
                "target": preference_ref,
                "rel": "refs",
                "strength": "strong",
                "provenance": null,
                "via": null,
                "anchor": null,
            }));
    }
    Ok(())
}

#[tauri::command(async)]
pub(crate) fn work_delivery_accept(
    project_dir: String,
    unit_id: String,
    source_sequence: i64,
) -> Result<kanzei_core::store::decisions::WorkAcceptance, String> {
    let root = crate::normalized_project_root(Path::new(&project_dir));
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
        .map_err(|e| e.to_string())?;
    store
        .accept_work_delivery(
            &kanzei_core::project_session_id(&root),
            &unit_id,
            source_sequence,
        )
        .map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn save_decision_review(
    project_dir: String,
    decision_id: String,
    review: ReviewDecision,
) -> Result<serde_json::Value, String> {
    let root = crate::normalized_project_root(Path::new(&project_dir));
    let _guard = kanzei_tools::atomic_file::lock_exclusive(&root.join(".kanzei/decision-reviews"))
        .map_err(|e| e.to_string())?;
    let store = kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root))
        .map_err(|e| e.to_string())?;
    let journal = kanzei_core::project_session_id(&root);
    let mut decision = store
        .review_decision(&journal, &decision_id, &review)
        .map_err(|e| e.to_string())?;
    let preference_error = match sync_preference(&root, &decision) {
        Ok(Some(id)) => {
            match store.link_decision_preference(&journal, &decision_id, &review.request_id, &id) {
                Ok(linked) => {
                    decision = linked;
                    None
                }
                Err(error) => Some(error.to_string()),
            }
        }
        Ok(None) => None,
        Err(error) => Some(error.to_string()),
    };
    Ok(
        serde_json::json!({"decision": decision, "preference_error": preference_error, "delivery": null}),
    )
}

fn sync_preference(root: &Path, decision: &DecisionRecord) -> anyhow::Result<Option<String>> {
    let Some(review) = &decision.review else {
        return Ok(None);
    };
    if review.action != ReviewAction::Correct || review.scope == PreferenceScope::Once {
        return Ok(None);
    }
    if let Some(id) = &review.preference_id {
        return Ok(Some(id.clone()));
    }
    let memory = match review.scope {
        PreferenceScope::Project => MemoryStore::project(root),
        PreferenceScope::Global => {
            MemoryStore::global().ok_or_else(|| anyhow::anyhow!("全局记忆目录不可用"))?
        }
        PreferenceScope::Once => unreachable!(),
    };
    // 跨项目写同一全局偏好时也串行，不能让旧的重试覆盖另一个窗口的新选择。
    std::fs::create_dir_all(&memory.root)?;
    let _guard =
        kanzei_tools::atomic_file::lock_exclusive(&memory.root.join("decision-preferences"))?;
    let subject =
        kanzei_core::store::decisions::decision_id("preference", "question", &decision.question);
    let marker = format!("decision-review:{}:{}", decision.id, review.request_id);
    let body = format!(
        "{}\n\n适用情境：{}\n用户显式选择范围：{:?}\n决策来源：{}\n复核标记：{}\n复核时间：{}",
        review.feedback, decision.question, review.scope, decision.id, marker, review.reviewed_at
    );
    let title = format!(
        "决策偏好：{}",
        decision.question.chars().take(60).collect::<String>()
    );
    if let Some((_, existing)) = memory
        .load_all()
        .into_iter()
        .find(|(_, e)| e.field("subject") == Some(subject.as_str()))
    {
        if existing.body.contains(&marker) {
            return Ok(Some(existing.id));
        }
        let newer = existing
            .body
            .lines()
            .filter_map(|line| line.strip_prefix("复核时间："))
            .filter_map(|s| s.parse::<i64>().ok())
            .any(|at| at > review.reviewed_at);
        anyhow::ensure!(!newer, "同一问题已有更新的偏好；本次旧复核没有覆盖它");
        let entry = memory.update(
            &existing.id,
            Some(&title),
            Some(&decision.question),
            Some(&body),
            Some("active"),
            None,
            false,
        )?;
        return Ok(Some(entry.id));
    }
    match memory.add(
        "preference",
        &title,
        &decision.question,
        &body,
        "user",
        std::slice::from_ref(&decision.id),
        Some(&subject),
        true,
    )? {
        AddOutcome::Added(entry) => Ok(Some(entry.id)),
        AddOutcome::Duplicate(entry) | AddOutcome::SubjectConflict(entry)
            if entry.body.contains(&marker) =>
        {
            Ok(Some(entry.id))
        }
        _ => anyhow::bail!("已有相似偏好，未覆盖。请到记忆页合并，纠正任务已保留。"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanzei_core::store::decisions::{AgentDecision, DecisionResolution};
    #[test]
    fn review_uses_original_session_and_user_scope_with_retry() {
        let root = std::env::temp_dir().join(format!(
            "kanzei-review-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let root = crate::normalized_project_root(&root);
        let store =
            kanzei_core::SessionStore::open(&kanzei_core::project_state_path(&root)).unwrap();
        let journal = kanzei_core::project_session_id(&root);
        let origin = format!("{journal}#p2");
        let project = root.display().to_string();
        store.create_session(&journal, &project, None).unwrap();
        store.create_session(&origin, &project, None).unwrap();
        let decision = store
            .record_agent_decision(
                &journal,
                AgentDecision {
                    project: &project,
                    session_id: &origin,
                    process_id: Some("p2"),
                    run_id: "run-test",
                    call_id: "call-test",
                    question: "验收节奏如何安排？",
                    options: vec![],
                    work_unit_id: None,
                    missing_fact: None,
                    resolution: Some(DecisionResolution {
                        answer: "每项人工验收".into(),
                        rationale: "初始选择".into(),
                        impact: "影响开发速度".into(),
                        preference_refs: vec![],
                    }),
                },
            )
            .unwrap();
        let review = ReviewDecision {
            request_id: "review-once".into(),
            expected_revision: decision.revision,
            action: ReviewAction::Correct,
            feedback: "自动验证后持续推进，用户集中验收".into(),
            scope: PreferenceScope::Once,
        };
        let result =
            save_decision_review(project.clone(), decision.id.clone(), review.clone()).unwrap();
        assert!(result["decision"]["review"]["preference_id"].is_null());
        assert!(MemoryStore::project(&root).load_all().is_empty());
        let review = ReviewDecision {
            request_id: "review-project".into(),
            expected_revision: result["decision"]["revision"].as_i64().unwrap(),
            scope: PreferenceScope::Project,
            ..review
        };
        let result =
            save_decision_review(project.clone(), decision.id.clone(), review.clone()).unwrap();
        assert!(result["preference_error"].is_null(), "{result}");
        let repeated = save_decision_review(project.clone(), decision.id.clone(), review).unwrap();
        assert_eq!(repeated, result);
        crate::ipc_contract::tests::check_contract(
            "decision_review",
            crate::ipc_contract::shape(&result),
            "自主决策复核的 IPC 契约",
        );
        let entries = MemoryStore::project(&root).load_all();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].1.status, "active");
        assert_eq!(entries[0].1.source, "user");
        assert_eq!(store.list_pending_inputs(&origin).unwrap().len(), 2);
        assert!(store.list_pending_inputs(&journal).unwrap().is_empty());
        let adopted = store
            .record_agent_decision(
                &journal,
                AgentDecision {
                    project: &project,
                    session_id: &origin,
                    process_id: Some("p2"),
                    run_id: "run-preference-adoption",
                    call_id: "call-preference-adoption",
                    question: "本轮是否沿用已确认偏好？",
                    options: vec![],
                    work_unit_id: None,
                    missing_fact: None,
                    resolution: Some(DecisionResolution {
                        answer: "沿用项目偏好".into(),
                        rationale: "引用此前用户确认的偏好".into(),
                        impact: "维持跨轮一致性".into(),
                        preference_refs: vec![entries[0].1.id.clone()],
                    }),
                },
            )
            .unwrap();
        let mut graph = crate::memory::memory_graph_with(&root, &[MemoryStore::project(&root)]);
        assert!(graph["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["id"] == decision.id && n["kind"] == "decision"));
        assert!(graph["edges"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["source"] == entries[0].1.id
                && e["target"] == decision.id
                && e["rel"] == "derived_from"));
        let adoption_edge_count = |graph: &serde_json::Value| {
            graph["edges"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|edge| {
                    edge["source"] == adopted.id
                        && edge["target"] == entries[0].1.id
                        && edge["rel"] == "refs"
                })
                .count()
        };
        assert_eq!(adoption_edge_count(&graph), 1);
        assert!(graph["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|node| node["id"] == adopted.id && node["kind"] == "decision"));

        // 图缓存重建以外重复叠加同一事实也不重复画边。
        crate::decisions::append_graph_lineage(&root, &mut graph).unwrap();
        assert_eq!(adoption_edge_count(&graph), 1);

        // project/global 同 id 有歧义时，以及目标不再 active 时，均不连 adoption 边。
        let mut ambiguous = graph.clone();
        let mut global_copy = ambiguous["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["id"] == entries[0].1.id && node["scope"] == "project")
            .unwrap()
            .clone();
        global_copy["scope"] = serde_json::json!("global");
        ambiguous["nodes"].as_array_mut().unwrap().push(global_copy);
        ambiguous["edges"].as_array_mut().unwrap().retain(|edge| {
            !(edge["source"] == adopted.id
                && edge["target"] == entries[0].1.id
                && edge["rel"] == "refs")
        });
        crate::decisions::append_graph_lineage(&root, &mut ambiguous).unwrap();
        assert_eq!(adoption_edge_count(&ambiguous), 0);

        let mut archived = graph.clone();
        archived["edges"].as_array_mut().unwrap().retain(|edge| {
            !(edge["source"] == adopted.id
                && edge["target"] == entries[0].1.id
                && edge["rel"] == "refs")
        });
        let preference = archived["nodes"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|node| node["id"] == entries[0].1.id && node["scope"] == "project")
            .unwrap();
        preference["status"] = serde_json::json!("archived");
        crate::decisions::append_graph_lineage(&root, &mut archived).unwrap();
        assert_eq!(adoption_edge_count(&archived), 0);

        // 同一接口契约也覆盖人工验收输出，机器完成不依赖此调用。
        use kanzei_core::{WorkEvidence, WorkFact, WorkUnitSpec};
        store
            .create_work_unit(WorkUnitSpec {
                unit_id: "R-900/W1".into(),
                requirement_id: "R-900".into(),
                objective: "验证交付入口".into(),
                scope: vec!["src".into()],
                dependencies: vec![],
                acceptance: vec!["works".into()],
                verification: vec!["targeted test".into()],
                base_revision: "abc123".into(),
            })
            .unwrap();
        store
            .append_work_fact("R-900/W1", WorkFact::Claimed { claimed_by: None })
            .unwrap();
        store
            .append_work_fact("R-900/W1", WorkFact::VerificationStarted)
            .unwrap();
        store
            .append_work_fact(
                "R-900/W1",
                WorkFact::EvidenceAdded {
                    evidence: WorkEvidence {
                        criterion: "works".into(),
                        evidence_refs: vec!["T-1".into()],
                    },
                },
            )
            .unwrap();
        let completed = store
            .append_work_fact("R-900/W1", WorkFact::Completed)
            .unwrap();
        let accepted = work_delivery_accept(
            root.display().to_string(),
            "R-900/W1".into(),
            completed.source_sequence,
        )
        .unwrap();
        crate::ipc_contract::tests::check_contract(
            "work_delivery_accept",
            crate::ipc_contract::shape(&serde_json::to_value(accepted).unwrap()),
            "人工验收 IPC 契约",
        );
        let job_id = "v-11111111111111111111111111111111";
        std::fs::create_dir_all(root.join(".kanzei/verification")).unwrap();
        std::fs::write(root.join(format!(".kanzei/verification/{job_id}.json")), serde_json::to_vec(&serde_json::json!({
            "id": job_id, "project": root.canonicalize().unwrap(), "source": root, "snapshot": root,
            "unit_id": "R-900/W1", "command": "fixture", "criteria": ["works"], "resource": "fixture", "environment": "test",
            "platform": "test", "source_fingerprint": "fixture", "manifest": [], "status": "queued", "created_at": 1, "updated_at": 1,
            "timeout_ms": 1000, "log_path": root.join("output.log"), "exit_code": null, "error": null
        })).unwrap()).unwrap();
        verification_cancel(root.display().to_string(), job_id.into()).unwrap();
        assert!(root
            .join(format!(".kanzei/verification/{job_id}.cancel"))
            .exists());
        crate::ipc_contract::tests::check_contract(
            "verification_cancel",
            crate::ipc_contract::shape(&serde_json::Value::Null),
            "验证取消 IPC 契约",
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[tauri::command(async)]
pub(crate) fn decision_review(
    window: tauri::Window,
    state: tauri::State<'_, crate::AppState>,
    project_dir: String,
    decision_id: String,
    review: ReviewDecision,
    agent: Option<String>,
) -> Result<serde_json::Value, String> {
    let mut result = save_decision_review(project_dir.clone(), decision_id, review)?;
    let decision: DecisionRecord =
        serde_json::from_value(result["decision"].clone()).map_err(|e| e.to_string())?;
    if let Some(input_id) = decision
        .review
        .as_ref()
        .and_then(|r| r.rework_input_id.clone())
    {
        let root = crate::normalized_project_root(Path::new(&project_dir));
        let delivery =
            crate::processes::restore_processes_from_store_once(&state, &root).and_then(|()| {
                crate::commands::run::schedule_run(
                    window,
                    &state,
                    project_dir,
                    decision.process_id.clone(),
                    crate::commands::run::Submission::Saved {
                        session_id: decision.session_id.clone(),
                        input_id: input_id.clone(),
                    },
                    crate::commands::run::RunOptions {
                        agent,
                        ..Default::default()
                    },
                )
            });
        result["delivery"] = match delivery {
            Ok(status) => {
                serde_json::json!({"status": status, "input_id": input_id, "session_id": decision.session_id, "error": null})
            }
            Err(error) => {
                serde_json::json!({"status": "saved", "input_id": input_id, "session_id": decision.session_id, "error": error})
            }
        };
    }
    Ok(result)
}
