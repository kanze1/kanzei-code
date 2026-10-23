use super::*;

fn spec(n: u32, parent: Option<&str>) -> WorkUnitSpec {
    WorkUnitSpec {
        unit_id: format!("R-1/W{n}"),
        requirement_id: "R-1".into(),
        objective: format!("work {n}"),
        scope: vec![],
        dependencies: vec![],
        acceptance: vec!["works".into()],
        verification: vec![],
        base_revision: "fixture".into(),
        node: WorkNodeContract {
            parent_id: parent.map(str::to_owned),
            covers: parent.map(|_| vec!["works".into()]).unwrap_or_default(),
            ..Default::default()
        },
    }
}
fn graph(store: &SessionStore) -> Vec<WorkProjection> {
    store.list_work_units(None).unwrap()
}
fn node(store: &SessionStore, id: &str) -> WorkProjection {
    store.get_work_unit(id).unwrap().unwrap()
}
fn claim(store: &SessionStore, id: &str) {
    store
        .append_work_fact(
            id,
            WorkFact::Claimed {
                claimed_by: Some("worker".into()),
            },
        )
        .unwrap();
}
fn finish(store: &SessionStore, id: &str) {
    claim(store, id);
    store
        .append_work_fact(id, WorkFact::VerificationStarted)
        .unwrap();
    store
        .append_work_fact(
            id,
            WorkFact::EvidenceAdded {
                evidence: WorkEvidence {
                    criterion: "works".into(),
                    evidence_refs: vec!["proof.txt".into()],
                },
            },
        )
        .unwrap();
    store.append_work_fact(id, WorkFact::Completed).unwrap();
}
fn decision(id: &str, disposition: WorkRevisionDisposition) -> WorkRevisionDecision {
    WorkRevisionDecision {
        unit_id: id.into(),
        disposition,
        reason: "compare the result with the new direction".into(),
        contract: None,
        covers: None,
        replacements: vec![],
    }
}

#[test]
fn independent_reviews_can_resolve_while_other_branches_remain_pending() {
    let store = SessionStore::open_in_memory().unwrap();
    root_and_children(&store);
    store.create_work_unit(spec(8, None)).unwrap();
    let first = store
        .begin_work_revision(
            "R-1/W2",
            &versions(&graph(&store)),
            "direction one",
            "reason",
        )
        .unwrap();
    let second = store
        .begin_work_revision(
            "R-1/W8",
            &versions(&graph(&store)),
            "direction two",
            "reason",
        )
        .unwrap();
    store
        .apply_work_revision(
            "R-1/W8",
            &second.revision_id,
            &versions(&graph(&store)),
            vec![decision("R-1/W8", WorkRevisionDisposition::Retain)],
            vec![],
        )
        .unwrap();
    assert!(node(&store, "R-1/W1").review_pending());
    assert_eq!(node(&store, "R-1/W2").review.unwrap().request, first);
    assert!(!node(&store, "R-1/W8").review_pending());
}

#[test]
fn reverify_requires_unfinished_successor_and_retirement_preserves_history() {
    let store = SessionStore::open_in_memory().unwrap();
    store.create_work_unit(spec(1, None)).unwrap();
    store.create_work_unit(spec(2, None)).unwrap();
    finish(&store, "R-1/W1");
    finish(&store, "R-1/W2");
    let revision = store
        .begin_work_revision(
            "R-1/W1",
            &versions(&graph(&store)),
            "new direction",
            "reason",
        )
        .unwrap();
    let mut reverify = decision("R-1/W1", WorkRevisionDisposition::Reverify);
    reverify.replacements = vec!["R-1/W2".into()];
    assert!(store
        .apply_work_revision(
            "R-1/W1",
            &revision.revision_id,
            &versions(&graph(&store)),
            vec![reverify],
            vec![]
        )
        .is_err());
    store
        .apply_work_revision(
            "R-1/W1",
            &revision.revision_id,
            &versions(&graph(&store)),
            vec![decision("R-1/W1", WorkRevisionDisposition::Retire)],
            vec![],
        )
        .unwrap();
    let retired = node(&store, "R-1/W1");
    assert_eq!(retired.status, WorkUnitStatus::Done);
    assert_eq!(retired.evidence.len(), 1);
    assert!(!retired.result_is_current());
    let mut dependent = spec(3, None);
    dependent.dependencies = vec![retired.unit_id];
    assert!(store.create_work_unit(dependent).is_err());
}

#[test]
fn user_acceptance_and_workspace_counts_follow_result_applicability() {
    let store = SessionStore::open_in_memory().unwrap();
    store.create_session("journal", "project", None).unwrap();
    store.create_work_unit(spec(1, None)).unwrap();
    finish(&store, "R-1/W1");
    assert_eq!(
        store.workspace_summary("journal").unwrap()["ready_to_try"],
        1
    );
    for disposition in [
        WorkRevisionDisposition::Retain,
        WorkRevisionDisposition::Retire,
    ] {
        let request = store
            .begin_work_revision(
                "R-1/W1",
                &versions(&graph(&store)),
                "new direction",
                "reason",
            )
            .unwrap();
        let pending = node(&store, "R-1/W1");
        assert!(!pending.result_is_current());
        assert_eq!(
            store.workspace_summary("journal").unwrap()["ready_to_try"],
            0
        );
        assert!(store
            .accept_work_delivery("journal", &pending.unit_id, pending.source_sequence)
            .is_err());
        store
            .apply_work_revision(
                "R-1/W1",
                &request.revision_id,
                &versions(&graph(&store)),
                vec![decision("R-1/W1", disposition)],
                vec![],
            )
            .unwrap();
        let resolved = node(&store, "R-1/W1");
        assert_eq!(
            store.workspace_summary("journal").unwrap()["ready_to_try"],
            u64::from(resolved.result_is_current())
        );
        assert_eq!(
            store
                .accept_work_delivery("journal", &resolved.unit_id, resolved.source_sequence)
                .is_ok(),
            resolved.result_is_current()
        );
        assert_eq!(
            store.workspace_summary("journal").unwrap()["recent_progress"]["kind"],
            "work.review_resolved"
        );
    }
}
fn root_and_children(store: &SessionStore) {
    let root = store.create_work_unit(spec(1, None)).unwrap();
    store
        .decompose_work_unit(
            &root.unit_id,
            root.source_sequence,
            "split",
            vec![spec(2, Some(&root.unit_id)), spec(3, Some(&root.unit_id))],
        )
        .unwrap();
}

#[test]
fn impact_traverses_consumers_and_integration_without_freezing_unrelated_siblings() {
    let store = SessionStore::open_in_memory().unwrap();
    root_and_children(&store);
    finish(&store, "R-1/W2");
    let mut consumer = spec(4, None);
    consumer.dependencies = vec!["R-1/W2".into()];
    let consumer = store.create_work_unit(consumer).unwrap();
    store
        .decompose_work_unit(
            &consumer.unit_id,
            consumer.source_sequence,
            "split consumer",
            vec![spec(5, Some(&consumer.unit_id))],
        )
        .unwrap();
    store.create_work_unit(spec(6, None)).unwrap();
    let impact = store.work_revision_impact("R-1/W2").unwrap();
    assert_eq!(
        impact
            .affected
            .iter()
            .map(|node| node.unit_id.as_str())
            .collect::<Vec<_>>(),
        vec!["R-1/W1", "R-1/W2", "R-1/W4", "R-1/W5"]
    );
    let evidence = node(&store, "R-1/W2").evidence;
    store
        .begin_work_revision(
            "R-1/W2",
            &impact.versions,
            "fully local memory",
            "user changes direction",
        )
        .unwrap();
    let done = node(&store, "R-1/W2");
    assert_eq!(done.status, WorkUnitStatus::Done);
    assert_eq!(done.evidence, evidence);
    assert!(!done.result_is_current());
    assert_eq!(done.readiness(&graph(&store)).step, WorkNodeStep::Wait);
    assert!(!node(&store, "R-1/W3").review_pending());
    claim(&store, "R-1/W3");
    claim(&store, "R-1/W6");
    let mut unsafe_dependency = spec(7, None);
    unsafe_dependency.dependencies = vec![done.unit_id];
    assert!(store.create_work_unit(unsafe_dependency).is_err());
    assert!(store.get_work_unit("R-1/W7").unwrap().is_none());
    assert!(store
        .append_work_fact("R-1/W5", WorkFact::Claimed { claimed_by: None })
        .is_err());
}

#[test]
fn batch_revision_is_atomic_and_reattests_completed_coverage_without_erasing_evidence() {
    let store = SessionStore::open_in_memory().unwrap();
    root_and_children(&store);
    finish(&store, "R-1/W2");
    let prior = node(&store, "R-1/W2");
    let request = store
        .begin_work_revision(
            "R-1/W1",
            &versions(&graph(&store)),
            "local operation",
            "change direction",
        )
        .unwrap();
    let before = graph(&store);
    let expected = versions(&before);
    let events = store.list_work_events("R-1/W1").unwrap();
    let mut parent = decision("R-1/W1", WorkRevisionDisposition::Revise);
    let mut contract = node(&store, "R-1/W1").spec();
    contract.acceptance = vec!["local".into()];
    parent.contract = Some(contract);
    let mut done = decision("R-1/W2", WorkRevisionDisposition::Retain);
    let mut child = decision("R-1/W3", WorkRevisionDisposition::Revise);
    let mut contract = node(&store, "R-1/W3").spec();
    contract.node.covers = vec!["local".into()];
    child.contract = Some(contract);
    assert!(
        store
            .apply_work_revision(
                "R-1/W1",
                &request.revision_id,
                &expected,
                vec![parent.clone(), done.clone(), child.clone()],
                vec![]
            )
            .is_err(),
        "retained coverage must explicitly support the new parent contract"
    );
    assert_eq!(graph(&store), before);
    assert_eq!(store.list_work_events("R-1/W1").unwrap(), events);
    done.covers = Some(vec!["local".into()]);
    let mut addition = spec(4, Some("R-1/W1"));
    addition.node.covers = vec!["local".into()];
    store
        .apply_work_revision(
            "R-1/W1",
            &request.revision_id,
            &expected,
            vec![parent, done, child],
            vec![addition],
        )
        .unwrap();
    let retained = node(&store, "R-1/W2");
    assert!(retained.result_is_current());
    assert_eq!(retained.acceptance, prior.acceptance);
    assert_eq!(retained.evidence, prior.evidence);
    assert_eq!(retained.node.covers, vec!["local"]);
    assert_eq!(
        node(&store, "R-1/W1")
            .review
            .unwrap()
            .resolution
            .unwrap()
            .created_ids,
        vec!["R-1/W4"]
    );
    assert_eq!(
        project_work_events(&store.list_work_events(&retained.unit_id).unwrap()).unwrap(),
        retained
    );
    assert!(store
        .apply_work_revision("R-1/W1", &request.revision_id, &expected, vec![], vec![])
        .is_err());
}

#[test]
fn read_set_detects_phantoms_and_overlapping_revisions_leave_no_partial_freeze() {
    let store = SessionStore::open_in_memory().unwrap();
    root_and_children(&store);
    let stale = store.work_revision_impact("R-1/W2").unwrap();
    let mut consumer = spec(4, None);
    consumer.dependencies = vec!["R-1/W2".into()];
    store.create_work_unit(consumer).unwrap();
    assert!(store
        .begin_work_revision("R-1/W2", &stale.versions, "new direction", "reason")
        .is_err());
    assert!(graph(&store).iter().all(|node| !node.review_pending()));
    let request = store
        .begin_work_revision(
            "R-1/W2",
            &versions(&graph(&store)),
            "new direction",
            "reason",
        )
        .unwrap();
    let before = graph(&store);
    assert!(store
        .begin_work_revision("R-1/W1", &versions(&before), "other direction", "overlap")
        .is_err());
    assert_eq!(graph(&store), before);
    let decisions = before
        .iter()
        .filter(|node| node.review_pending())
        .map(|node| decision(&node.unit_id, WorkRevisionDisposition::Retain))
        .collect::<Vec<_>>();
    assert!(store
        .apply_work_revision(
            "R-1/W2",
            &request.revision_id,
            &versions(&before),
            decisions[..decisions.len() - 1].to_vec(),
            vec![]
        )
        .is_err());
    store.create_work_unit(spec(5, None)).unwrap();
    assert!(store
        .apply_work_revision(
            "R-1/W2",
            &request.revision_id,
            &versions(&before),
            decisions.clone(),
            vec![]
        )
        .is_err());
    store
        .apply_work_revision(
            "R-1/W2",
            &request.revision_id,
            &versions(&graph(&store)),
            decisions,
            vec![],
        )
        .unwrap();
}

#[test]
fn running_verification_must_finish_and_executor_yield_before_replanning() {
    let store = SessionStore::open_in_memory().unwrap();
    store.create_work_unit(spec(1, None)).unwrap();
    claim(&store, "R-1/W1");
    store
        .append_work_fact(
            "R-1/W1",
            WorkFact::VerificationQueued {
                job_id: "job".into(),
                snapshot_fingerprint: "snapshot".into(),
            },
        )
        .unwrap();
    let request = store
        .begin_work_revision(
            "R-1/W1",
            &versions(&graph(&store)),
            "new direction",
            "reason",
        )
        .unwrap();
    let keep = decision("R-1/W1", WorkRevisionDisposition::Retain);
    assert!(store
        .apply_work_revision(
            "R-1/W1",
            &request.revision_id,
            &versions(&graph(&store)),
            vec![keep.clone()],
            vec![]
        )
        .is_err());
    assert!(store
        .append_work_fact(
            "R-1/W1",
            WorkFact::RevisionYielded {
                revision_id: request.revision_id.clone(),
                reason: "stop".into()
            }
        )
        .is_err());
    assert!(store
        .append_work_fact(
            "R-1/W1",
            WorkFact::EvidenceAdded {
                evidence: WorkEvidence {
                    criterion: "works".into(),
                    evidence_refs: vec!["old-proof".into()]
                }
            }
        )
        .is_err());
    store
        .append_work_fact(
            "R-1/W1",
            WorkFact::VerificationConcluded {
                job_id: "job".into(),
                passed: true,
                reason: String::new(),
            },
        )
        .unwrap();
    assert!(store
        .append_work_fact("R-1/W1", WorkFact::Completed)
        .is_err());
    assert!(store
        .append_work_fact(
            "R-1/W1",
            WorkFact::RevisionYielded {
                revision_id: "wrong".into(),
                reason: "stop".into()
            }
        )
        .is_err());
    store
        .append_work_fact(
            "R-1/W1",
            WorkFact::RevisionYielded {
                revision_id: request.revision_id.clone(),
                reason: "executor acknowledges safe checkpoint".into(),
            },
        )
        .unwrap();
    assert_eq!(node(&store, "R-1/W1").claimed_by, None);
    store
        .apply_work_revision(
            "R-1/W1",
            &request.revision_id,
            &versions(&graph(&store)),
            vec![keep],
            vec![],
        )
        .unwrap();
    assert_eq!(node(&store, "R-1/W1").status, WorkUnitStatus::Ready);
}

#[test]
fn revalidation_preserves_done_history_but_requires_consumers_to_use_the_successor() {
    let store = SessionStore::open_in_memory().unwrap();
    root_and_children(&store);
    finish(&store, "R-1/W2");
    let current = node(&store, "R-1/W3");
    let mut contract = current.spec();
    contract.dependencies = vec!["R-1/W2".into()];
    store
        .refine_work_unit(contract, current.source_sequence, "consume research")
        .unwrap();
    let old = node(&store, "R-1/W2");
    let request = store
        .begin_work_revision(
            "R-1/W2",
            &versions(&graph(&store)),
            "recheck local constraints",
            "new evidence",
        )
        .unwrap();
    let mut redo = decision("R-1/W2", WorkRevisionDisposition::Reverify);
    redo.replacements = vec!["R-1/W4".into()];
    let parent = decision("R-1/W1", WorkRevisionDisposition::Retain);
    let consumer = decision("R-1/W3", WorkRevisionDisposition::Retain);
    let successor = spec(4, Some("R-1/W1"));
    let expected = versions(&graph(&store));
    assert!(store
        .apply_work_revision(
            "R-1/W2",
            &request.revision_id,
            &expected,
            vec![parent.clone(), redo.clone(), consumer],
            vec![successor.clone()]
        )
        .is_err());
    assert!(store.get_work_unit("R-1/W4").unwrap().is_none());
    let mut consumer = decision("R-1/W3", WorkRevisionDisposition::Revise);
    let mut contract = node(&store, "R-1/W3").spec();
    contract.dependencies = vec!["R-1/W4".into()];
    consumer.contract = Some(contract);
    store
        .apply_work_revision(
            "R-1/W2",
            &request.revision_id,
            &expected,
            vec![parent, redo, consumer],
            vec![successor],
        )
        .unwrap();
    let obsolete = node(&store, "R-1/W2");
    assert_eq!(obsolete.status, WorkUnitStatus::Done);
    assert_eq!(obsolete.evidence, old.evidence);
    assert!(!obsolete.result_is_current());
    assert!(!node(&store, "R-1/W3").dependencies_satisfied(&graph(&store)));
    finish(&store, "R-1/W4");
    assert!(node(&store, "R-1/W3").dependencies_satisfied(&graph(&store)));
}

#[test]
fn final_graph_cycles_roll_back_all_revisions() {
    let store = SessionStore::open_in_memory().unwrap();
    root_and_children(&store);
    let request = store
        .begin_work_revision(
            "R-1/W1",
            &versions(&graph(&store)),
            "new direction",
            "reason",
        )
        .unwrap();
    let before = graph(&store);
    let mut decisions = vec![decision("R-1/W1", WorkRevisionDisposition::Retain)];
    for (id, dependency) in [("R-1/W2", "R-1/W3"), ("R-1/W3", "R-1/W2")] {
        let mut choice = decision(id, WorkRevisionDisposition::Revise);
        let mut spec = node(&store, id).spec();
        spec.dependencies = vec![dependency.into()];
        choice.contract = Some(spec);
        decisions.push(choice);
    }
    assert!(store
        .apply_work_revision(
            "R-1/W1",
            &request.revision_id,
            &versions(&before),
            decisions,
            vec![]
        )
        .is_err());
    assert_eq!(graph(&store), before);
}

#[test]
fn review_migration_and_restart_preserve_original_completed_events() {
    let dir = kanzei_base::test_support::TestDir::new("work-review-upgrade-");
    let path = dir.join("state.sqlite");
    let old_events;
    {
        let store = SessionStore::open(&path).unwrap();
        store.create_work_unit(spec(1, None)).unwrap();
        finish(&store, "R-1/W1");
        old_events = store.list_work_events("R-1/W1").unwrap();
        store.connection.execute_batch("UPDATE work_surfaces SET projection_json=json_set(projection_json,'$.format_version',4); UPDATE schema_meta SET value='32' WHERE key='schema_version';").unwrap();
    }
    let store = SessionStore::open(&path).unwrap();
    assert!(store.backup_path(32).unwrap().is_file());
    assert_eq!(store.list_work_events("R-1/W1").unwrap(), old_events);
    store
        .begin_work_revision(
            "R-1/W1",
            &versions(&graph(&store)),
            "new direction",
            "reason",
        )
        .unwrap();
    drop(store);
    let store = SessionStore::open(&path).unwrap();
    let restored = node(&store, "R-1/W1");
    assert!(restored.review_pending());
    assert_eq!(restored.status, WorkUnitStatus::Done);
    assert_eq!(
        project_work_events(&store.list_work_events("R-1/W1").unwrap()).unwrap(),
        restored
    );
    assert_eq!(
        &store.list_work_events("R-1/W1").unwrap()[..old_events.len()],
        old_events.as_slice()
    );
}
