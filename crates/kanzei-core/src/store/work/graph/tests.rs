use super::*;

fn spec(n: usize, parent: Option<&str>, kind: WorkNodeKind) -> WorkUnitSpec {
    WorkUnitSpec {
        unit_id: format!("R-1/W{n}"),
        requirement_id: "R-1".into(),
        objective: format!("deliver {n}"),
        scope: vec![],
        dependencies: vec![],
        acceptance: vec!["works".into()],
        verification: vec![],
        base_revision: "fixture".into(),
        node: WorkNodeContract {
            kind,
            parent_id: parent.map(Into::into),
            covers: parent.map(|_| vec!["works".into()]).unwrap_or_default(),
            ..Default::default()
        },
    }
}

fn claim(store: &SessionStore, id: &str) -> WorkProjection {
    store
        .append_work_fact(
            id,
            WorkFact::Claimed {
                claimed_by: Some("main".into()),
            },
        )
        .unwrap()
}

fn finish(store: &SessionStore, id: &str) {
    claim(store, id);
    store
        .append_work_fact(id, WorkFact::VerificationStarted)
        .unwrap();
    assert!(store.append_work_fact(id, WorkFact::Completed).is_err());
    store
        .append_work_facts(
            id,
            vec![
                WorkFact::EvidenceAdded {
                    evidence: WorkEvidence {
                        criterion: "works".into(),
                        evidence_refs: vec!["test:observed-behaviour".into()],
                    },
                },
                WorkFact::Completed,
            ],
        )
        .unwrap();
}

#[test]
fn recursive_research_to_implementation_requires_parent_integration_and_replays() {
    let store = SessionStore::open_in_memory().unwrap();
    let mut root = spec(1, None, WorkNodeKind::Goal);
    root.node.questions.push("which design?".into());
    store.create_work_unit(root).unwrap();
    let active = claim(&store, "R-1/W1");
    assert!(store
        .append_work_fact("R-1/W1", WorkFact::VerificationStarted)
        .is_err());
    store
        .decompose_work_unit(
            &active.unit_id,
            active.source_sequence,
            "resolve knowledge first",
            vec![spec(2, Some("R-1/W1"), WorkNodeKind::Research)],
        )
        .unwrap();
    let parent = store.get_work_unit("R-1/W1").unwrap().unwrap();
    assert_eq!(parent.claimed_by, None);
    assert_eq!(parent.status, WorkUnitStatus::Ready);
    assert!(store
        .append_work_fact("R-1/W1", WorkFact::Claimed { claimed_by: None })
        .is_err());
    assert!(store
        .append_work_fact(
            "R-1/W1",
            WorkFact::Superseded {
                reason: "hide remaining work".into()
            }
        )
        .is_err());
    let child = claim(&store, "R-1/W2");
    store
        .decompose_work_unit(
            &child.unit_id,
            child.source_sequence,
            "need a concrete example",
            vec![spec(3, Some("R-1/W2"), WorkNodeKind::Verification)],
        )
        .unwrap();
    finish(&store, "R-1/W3");
    let research = store.get_work_unit("R-1/W2").unwrap().unwrap();
    assert_eq!(
        research
            .readiness(&store.list_work_units(None).unwrap())
            .step,
        WorkNodeStep::Integrate
    );
    finish(&store, "R-1/W2");
    let parent = store.get_work_unit("R-1/W1").unwrap().unwrap();
    assert_eq!(
        parent.readiness(&store.list_work_units(None).unwrap()).step,
        WorkNodeStep::Plan
    );
    let mut clarified = parent.spec();
    clarified.node.questions.clear();
    clarified.node.explanation = "the experiment supports the design".into();
    let clarified = store
        .refine_work_unit(
            clarified,
            parent.source_sequence,
            "research answered the question",
        )
        .unwrap();
    store
        .decompose_work_unit(
            &clarified.unit_id,
            clarified.source_sequence,
            "deliver the selected design",
            vec![spec(4, Some("R-1/W1"), WorkNodeKind::Requirement)],
        )
        .unwrap();
    finish(&store, "R-1/W4");
    finish(&store, "R-1/W1");
    for node in store.list_work_units(None).unwrap() {
        assert_eq!(store.rebuild_work_surface(&node.unit_id).unwrap(), node);
    }
}

#[test]
fn invalid_decomposition_rolls_back_children_events_and_parent_claim() {
    let store = SessionStore::open_in_memory().unwrap();
    store
        .create_work_unit(spec(1, None, WorkNodeKind::Goal))
        .unwrap();
    let parent = claim(&store, "R-1/W1");
    let good = spec(2, Some("R-1/W1"), WorkNodeKind::Research);
    let mut cyclic = spec(3, Some("R-1/W1"), WorkNodeKind::Defect);
    cyclic.dependencies.push("R-1/W1".into());
    let error = store
        .decompose_work_unit(
            &parent.unit_id,
            parent.source_sequence,
            "invalid plan",
            vec![good.clone(), cyclic],
        )
        .unwrap_err();
    assert!(error.to_string().contains("循环"));
    assert_eq!(store.list_work_units(None).unwrap(), vec![parent.clone()]);
    assert!(store.list_work_events("R-1/W2").unwrap().is_empty());
    assert_eq!(store.get_work_unit("R-1/W1").unwrap().unwrap(), parent);
    assert!(store
        .decompose_work_unit(
            &parent.unit_id,
            parent.source_sequence - 1,
            "stale plan",
            vec![good]
        )
        .unwrap_err()
        .to_string()
        .contains("版本"));
}

#[test]
fn ancestor_dependencies_apply_to_descendants_and_cannot_form_hidden_cycles() {
    let store = SessionStore::open_in_memory().unwrap();
    store
        .create_work_unit(spec(1, None, WorkNodeKind::Research))
        .unwrap();
    let mut root = spec(2, None, WorkNodeKind::Goal);
    root.dependencies.push("R-1/W1".into());
    let root = store.create_work_unit(root).unwrap();
    store
        .decompose_work_unit(
            &root.unit_id,
            root.source_sequence,
            "plan in advance",
            vec![spec(3, Some("R-1/W2"), WorkNodeKind::Requirement)],
        )
        .unwrap();
    assert!(store
        .append_work_fact("R-1/W3", WorkFact::Claimed { claimed_by: None })
        .is_err());
    let prerequisite = store.get_work_unit("R-1/W1").unwrap().unwrap();
    let mut bad = prerequisite.spec();
    bad.dependencies.push("R-1/W3".into());
    assert!(store
        .refine_work_unit(
            bad,
            prerequisite.source_sequence,
            "would deadlock descendants"
        )
        .unwrap_err()
        .to_string()
        .contains("循环"));
    finish(&store, "R-1/W1");
    finish(&store, "R-1/W3");
    finish(&store, "R-1/W2");
}

#[test]
fn refinement_invalidates_old_evidence_and_cancelled_children_do_not_cover_goals() {
    let store = SessionStore::open_in_memory().unwrap();
    store
        .create_work_unit(spec(1, None, WorkNodeKind::Goal))
        .unwrap();
    claim(&store, "R-1/W1");
    let parent = store
        .append_work_fact(
            "R-1/W1",
            WorkFact::EvidenceAdded {
                evidence: WorkEvidence {
                    criterion: "works".into(),
                    evidence_refs: vec!["old-report.md".into()],
                },
            },
        )
        .unwrap();
    let mut changed = parent.spec();
    changed.node.explanation = "corrected understanding".into();
    let parent = store
        .refine_work_unit(changed, parent.source_sequence, "new evidence")
        .unwrap();
    assert!(parent.evidence.is_empty());
    store
        .decompose_work_unit(
            &parent.unit_id,
            parent.source_sequence,
            "explore",
            vec![spec(2, Some("R-1/W1"), WorkNodeKind::Design)],
        )
        .unwrap();
    store
        .append_work_fact(
            "R-1/W2",
            WorkFact::Superseded {
                reason: "not viable".into(),
            },
        )
        .unwrap();
    let parent = store.get_work_unit("R-1/W1").unwrap().unwrap();
    let readiness = parent.readiness(&store.list_work_units(None).unwrap());
    assert_eq!(readiness.step, WorkNodeStep::Plan);
    assert_eq!(readiness.uncovered, vec!["works"]);
    claim(&store, "R-1/W1");
    assert!(store
        .append_work_fact("R-1/W1", WorkFact::VerificationStarted)
        .is_err());
    assert!(store
        .append_work_fact(
            "R-1/W1",
            WorkFact::EvidenceAdded {
                evidence: WorkEvidence {
                    criterion: "works".into(),
                    evidence_refs: vec![" ".into()]
                }
            }
        )
        .is_err());
}

#[test]
fn legacy_contract_defaults_without_rewriting_old_events() {
    let old = serde_json::json!({"unit_id":"R-1/W1", "requirement_id":"R-1", "objective":"old unit", "acceptance":["works"], "base_revision":"fixture"});
    let old: WorkUnitSpec = serde_json::from_value(old).unwrap();
    assert_eq!(old.node, WorkNodeContract::default());
    let store = SessionStore::open_in_memory().unwrap();
    store.create_work_unit(old).unwrap();
    finish(&store, "R-1/W1");
}

#[test]
fn recursive_schema_upgrade_preserves_legacy_events_and_creates_downgrade_backup() {
    let root = kanzei_base::test_support::TestDir::new("recursive-schema-upgrade-");
    let path = root.join("state.db");
    {
        let store = SessionStore::open(&path).unwrap();
        store
            .create_work_unit(spec(1, None, WorkNodeKind::Requirement))
            .unwrap();
        store.connection.execute_batch("UPDATE work_events SET payload_json=json_remove(payload_json,'$.spec.node'); UPDATE work_surfaces SET projection_json=json_set(json_remove(projection_json,'$.node'),'$.format_version',1); UPDATE schema_meta SET value='29' WHERE key='schema_version';").unwrap();
    }
    let store = SessionStore::open(&path).unwrap();
    let version: String = store
        .connection
        .query_row(
            "SELECT value FROM schema_meta WHERE key='schema_version'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version, "30");
    assert!(store.backup_path(29).unwrap().is_file());
    let legacy = store.get_work_unit("R-1/W1").unwrap().unwrap();
    assert_eq!(legacy.node, WorkNodeContract::default());
    let original: String = store
        .connection
        .query_row(
            "SELECT payload_json FROM work_events WHERE unit_id='R-1/W1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        !original.contains("\"node\""),
        "migration must not rewrite historical contracts"
    );
    store
        .decompose_work_unit(
            "R-1/W1",
            legacy.source_sequence,
            "extend a legacy unit",
            vec![spec(2, Some("R-1/W1"), WorkNodeKind::Research)],
        )
        .unwrap();
    drop(store);
    let restored = SessionStore::open(&path).unwrap();
    assert_eq!(
        restored
            .get_work_unit("R-1/W1")
            .unwrap()
            .unwrap()
            .readiness(&restored.list_work_units(None).unwrap())
            .step,
        WorkNodeStep::Wait
    );
}
