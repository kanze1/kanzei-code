//! Real CLI processes exercise persistent recursive planning without a provider substitute.
use super::common::TestHome;
use serde_json::{json, Value};
use std::process::Command;

fn run(home: &TestHome, root: &std::path::Path, args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_kz"))
        .args(args)
        .current_dir(root)
        .envs(home.envs())
        .env("KANZEI_PROJECT_ROOT", root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "args={args:?}\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn revision_cli_applies_one_versioned_graph_and_restarts_with_history() {
    let home = TestHome::new("revision-cli");
    let root = home.root.join("project");
    std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
    std::fs::write(root.join(".kanzei/project/requirements.md"), "# Requirements\n\n## R-001 Personal agent [doing]\n- 执行模型: work_units_v1\n- 目标: agent\n").unwrap();
    std::fs::write(root.join(".kanzei/project/defects.md"), "# Defects\n").unwrap();
    let unit = run(
        &home,
        &root,
        &[
            "work",
            "create-unit",
            "--requirement",
            "R-001",
            "--objective",
            "personal agent",
            "--acceptance",
            "works",
            "--base-revision",
            "fixture",
        ],
    );
    let id = unit["unit_id"].as_str().unwrap();
    let impact = run(&home, &root, &["work", "impact", id]);
    std::fs::write(root.join("versions.json"), impact["versions"].to_string()).unwrap();
    let request = run(
        &home,
        &root,
        &[
            "work",
            "begin-revision",
            id,
            "--versions-file",
            "versions.json",
            "--direction",
            "local first",
            "--reason",
            "user changed direction",
        ],
    );
    let selected = run(&home, &root, &["work", "next"]);
    assert_eq!(
        selected["selected"]["work_unit_context"]["revision_action"],
        "resolve"
    );
    let impact = run(&home, &root, &["work", "impact", id]);
    std::fs::write(root.join("versions.json"), impact["versions"].to_string()).unwrap();
    let mut contract = run(&home, &root, &["work", "get-unit", id])["contract"].clone();
    contract["objective"] = json!("local personal agent");
    std::fs::write(
        root.join("decisions.json"),
        json!([{"unit_id":id,"disposition":"revise","reason":"local scope","contract":contract}])
            .to_string(),
    )
    .unwrap();
    run(
        &home,
        &root,
        &[
            "work",
            "apply-revision",
            id,
            "--revision-id",
            request["revision_id"].as_str().unwrap(),
            "--versions-file",
            "versions.json",
            "--decisions-file",
            "decisions.json",
        ],
    );
    let restored = run(&home, &root, &["work", "get-unit", id]);
    assert_eq!(restored["unit"]["objective"], "local personal agent");
    assert_eq!(
        restored["unit"]["review"]["resolution"]["decision"]["disposition"],
        "revise"
    );
    assert_eq!(restored["events"].as_array().unwrap().len(), 3);
    assert_eq!(restored["unit"]["status"], "ready");
}

#[test]
fn recursive_cli_persists_graph_and_resumes_parent_after_restart() {
    let home = TestHome::new("recursive-cli");
    let root = home.root.join("project");
    std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
    std::fs::write(root.join(".kanzei/project/requirements.md"), "# Requirements\n\n## R-001 Personal agent [todo]\n- 执行模型: work_units_v1\n- 目标: Deliver a personal agent\n").unwrap();
    std::fs::write(root.join(".kanzei/project/defects.md"), "# Defects\n").unwrap();
    std::fs::write(
        root.join("node.json"),
        json!({"kind":"goal","questions":["which design?"]}).to_string(),
    )
    .unwrap();
    let initial = run(&home, &root, &["work", "next"]);
    assert_eq!(initial["selected"]["outcome_action"], "plan");
    let goal = run(
        &home,
        &root,
        &[
            "work",
            "create-unit",
            "--requirement",
            "R-001",
            "--objective",
            "personal agent",
            "--acceptance",
            "works",
            "--base-revision",
            "fixture",
            "--node-file",
            "node.json",
        ],
    );
    let id = goal["unit_id"].as_str().unwrap();
    // A UI selecting the Outcome is routed to the actual ready node, not a second WIP.
    let active = run(&home, &root, &["work", "claim", "R-001"]);
    assert_eq!(active["unit_id"], id);
    std::fs::write(root.join("children.json"), json!([
        {"key":"research","objective":"compare designs","acceptance":["works"],"node":{"kind":"research","covers":["works"]}},
        {"key":"implementation","objective":"implement design","acceptance":["works"],"dependencies":["@research"],"node":{"kind":"requirement","covers":["works"]}}
    ]).to_string()).unwrap();
    let sequence = active["source_sequence"].as_i64().unwrap().to_string();
    run(
        &home,
        &root,
        &[
            "work",
            "decompose",
            id,
            "--expected-sequence",
            &sequence,
            "--reason",
            "research determines implementation",
            "--children-file",
            "children.json",
        ],
    );
    std::fs::write(
        root.join("proof.txt"),
        "recorded deterministic scenario result",
    )
    .unwrap();
    for node in ["R-001/W2", "R-001/W3"] {
        let selected = run(&home, &root, &["work", "next"]);
        assert_eq!(selected["selected"]["id"], node);
        run(&home, &root, &["work", "claim", node]);
        run(&home, &root, &["work", "verify", node]);
        run(
            &home,
            &root,
            &[
                "work",
                "evidence",
                node,
                "--criterion",
                "works",
                "--evidence",
                "proof.txt",
            ],
        );
        run(&home, &root, &["work", "complete", node]);
    }
    let parent = run(&home, &root, &["work", "get-unit", id]);
    let mut spec = parent["contract"].clone();
    spec["node"]["questions"] = json!([]);
    spec["node"]["explanation"] = json!("design selected from research evidence");
    std::fs::write(root.join("refinement.json"), spec.to_string()).unwrap();
    let sequence = parent["unit"]["source_sequence"]
        .as_i64()
        .unwrap()
        .to_string();
    run(
        &home,
        &root,
        &[
            "work",
            "refine",
            id,
            "--expected-sequence",
            &sequence,
            "--reason",
            "research resolved question",
            "--refinement-file",
            "refinement.json",
        ],
    );
    let selection = run(&home, &root, &["work", "next"]);
    assert_eq!(
        selection["selected"]["work_unit_context"]["readiness"]["step"],
        "integrate"
    );
    run(&home, &root, &["work", "claim", id]);
    run(&home, &root, &["work", "verify", id]);
    run(
        &home,
        &root,
        &[
            "work",
            "evidence",
            id,
            "--criterion",
            "works",
            "--evidence",
            "proof.txt",
        ],
    );
    run(&home, &root, &["work", "complete", id]);
    let graph = run(&home, &root, &["work", "graph", "--requirement", "R-001"]);
    assert_eq!(graph["nodes"].as_array().unwrap().len(), 3);
    let next = run(&home, &root, &["work", "next"]);
    assert_eq!(next["selected"]["outcome_action"], "accept");
    assert_eq!(next["decision"], "start");
}
