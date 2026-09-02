//! R-349：active tracker 条目的机械对账投影。
//!
//! 这里把「声明的 commit / 源码指纹 → 当前 HEAD 与工作树 → 测试记录」收敛为
//! 只读报告。它不写 tracker，也不替代 close/verify；work next 将报告作为选中条目的
//! 可见提示，绝不把对账分类当作阻塞或重复取活的硬门禁。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use serde::Serialize;

use super::RepoObservation;
use crate::docstore::Entry;
use crate::test_record::records_for_entry;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReconcileClass {
    Stale,
    ImplementedUncommitted,
    CommittedUnverified,
    VerifiedUnclosed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReconcileItem {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub classification: ReconcileClass,
    pub reasons: Vec<String>,
    pub declared_commit: Option<String>,
    pub current_head: String,
    pub declared_source_fingerprint: Option<String>,
    pub evidence_source_fingerprints: Vec<String>,
    pub test_record_ids: Vec<String>,
    pub source_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReconciliationReport {
    pub items: Vec<ReconcileItem>,
    pub counts: BTreeMap<String, usize>,
}

impl ReconciliationReport {
    pub fn already_committed(&self, id: &str) -> bool {
        self.items.iter().any(|item| {
            item.id == id
                && matches!(
                    item.classification,
                    ReconcileClass::CommittedUnverified | ReconcileClass::VerifiedUnclosed
                )
        })
    }

    pub fn classification_reason(&self, id: &str) -> Option<String> {
        self.items.iter().find(|item| item.id == id).map(|item| {
            format!(
                "{}：{}",
                classification_name(item.classification),
                item.reasons.join("；")
            )
        })
    }
}

fn classification_name(classification: ReconcileClass) -> &'static str {
    match classification {
        ReconcileClass::Stale => "stale",
        ReconcileClass::ImplementedUncommitted => "implemented-uncommitted",
        ReconcileClass::CommittedUnverified => "committed-unverified",
        ReconcileClass::VerifiedUnclosed => "verified-unclosed",
    }
}

fn git_stdout(root: &Path, args: &[&str]) -> Option<String> {
    let mut command = Command::new("git");
    crate::hide_console(&mut command);
    let output = command.current_dir(root).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn current_head(root: &Path) -> Option<String> {
    git_stdout(root, &["rev-parse", "HEAD"]).filter(|head| !head.is_empty())
}

fn is_ancestor(root: &Path, commit: &str, head: &str) -> bool {
    let mut command = Command::new("git");
    crate::hide_console(&mut command);
    command
        .current_dir(root)
        .args(["merge-base", "--is-ancestor", commit, head])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn field(entry: &Entry, names: &[&str]) -> Option<String> {
    entry
        .fields
        .iter()
        .find(|(key, value)| {
            !value.trim().is_empty()
                && names
                    .iter()
                    .any(|name| key == name || key.eq_ignore_ascii_case(name))
        })
        .map(|(_, value)| value.trim().to_string())
}

fn commit_token(raw: &str) -> Option<String> {
    let token = raw.trim_matches(|ch: char| !ch.is_ascii_hexdigit());
    if (7..=40).contains(&token.len()) && token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Some(token.to_string())
    } else {
        None
    }
}

fn declared_commit(entry: &Entry) -> Option<String> {
    if let Some(value) = field(entry, &["observed_head", "commit", "commit_id", "提交"]) {
        return commit_token(&value);
    }
    let progress = field(entry, &["进展", "progress"])?;
    for marker in ["commit", "Commit", "提交", "HEAD"] {
        let Some(position) = progress.find(marker) else {
            continue;
        };
        let after = &progress[position + marker.len()..];
        for token in after.split_whitespace() {
            if let Some(commit) = commit_token(token) {
                return Some(commit);
            }
        }
    }
    None
}

fn declared_fingerprint(entry: &Entry) -> Option<String> {
    if let Some(value) = field(entry, &["源码指纹", "source_fingerprint", "fingerprint"]) {
        return Some(value);
    }
    let progress = field(entry, &["进展", "progress"])?;
    let start = progress.find("v2 ")?;
    let value = progress[start..]
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string();
    (!value.is_empty()).then_some(value)
}

fn is_source_path(path: &str) -> bool {
    let path = path.replace('\\', "/");
    !path.starts_with(".kanzei/")
        && !path.starts_with("docs/")
        && (path.starts_with("crates/")
            || path.starts_with("scripts/")
            || matches!(
                Path::new(&path).extension().and_then(|ext| ext.to_str()),
                Some("rs" | "js" | "mjs" | "css" | "html" | "toml" | "ps1")
            ))
}

fn changed_source_files(root: &Path, commit: &str, head: &str) -> Vec<String> {
    let Some(output) = git_stdout(
        root,
        &[
            "diff",
            "--name-only",
            commit,
            head,
            "--",
            ".",
            ":(exclude).kanzei/**",
        ],
    ) else {
        return Vec::new();
    };
    let mut paths = output
        .lines()
        .map(str::trim)
        .filter(|path| !path.is_empty() && is_source_path(path))
        .map(|path| path.replace('\\', "/"))
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    paths
}

fn changed_worktree_source_files(root: &Path, allowed: &[String]) -> Vec<String> {
    let allowed = allowed.iter().cloned().collect::<BTreeSet<_>>();
    let Some(output) = git_stdout(root, &["diff", "--name-only", "HEAD", "--", "."]) else {
        return Vec::new();
    };
    output
        .lines()
        .map(str::trim)
        .map(|path| path.replace('\\', "/"))
        .filter(|path| allowed.contains(path) && is_source_path(path))
        .collect()
}

fn ledger_scope(root: &Path, entry_id: &str) -> (bool, Vec<String>, Option<String>, Vec<String>) {
    let facts = crate::work::log::deliver_facts(root, entry_id);
    if facts.is_empty() {
        return (false, Vec::new(), None, Vec::new());
    }
    let mut paths = BTreeSet::new();
    let mut test_record_ids = BTreeSet::new();
    let mut commit = None;
    for fact in facts {
        paths.extend(
            fact.paths
                .into_iter()
                .map(|path| path.replace('\\', "/"))
                .filter(|path| is_source_path(path)),
        );
        test_record_ids.extend(fact.test_record_ids);
        if !fact.commit.trim().is_empty() {
            commit = Some(fact.commit);
        }
    }
    (
        true,
        paths.into_iter().collect(),
        commit,
        test_record_ids.into_iter().collect(),
    )
}

fn head_fingerprint_for_paths(root: &Path, paths: &[String]) -> Option<String> {
    if paths.is_empty() {
        return None;
    }
    let mut args = vec!["ls-tree", "-r", "--full-tree", "HEAD", "--"];
    args.extend(paths.iter().map(String::as_str));
    let output = git_stdout(root, &args)?;
    let mut entries = Vec::new();
    for line in output.lines() {
        let (meta, path) = line.split_once('\t')?;
        let sha = meta.split_whitespace().nth(2)?;
        entries.push(format!(
            "{}@{}",
            path.replace('\\', "/"),
            &sha[..sha.len().min(12)]
        ));
    }
    entries.sort();
    (!entries.is_empty()).then(|| format!("v2 {}", entries.join(",")))
}

fn record_field(record: &serde_json::Value, key: &str) -> Option<String> {
    record["fields"].as_array()?.iter().find_map(|field| {
        (field["key"].as_str() == Some(key))
            .then(|| field["value"].as_str().unwrap_or_default().to_string())
    })
}

fn passed_evidence(root: &Path, id: &str) -> (Vec<String>, Vec<String>) {
    let mut ids = Vec::new();
    let mut fingerprints = Vec::new();
    for record in records_for_entry(root, id) {
        if record["status"].as_str() != Some("passed") {
            continue;
        }
        if let Some(record_id) = record["id"].as_str() {
            ids.push(record_id.to_string());
        }
        if let Some(fingerprint) = record_field(&record, "源码指纹") {
            if !fingerprint.trim().is_empty() {
                fingerprints.push(fingerprint);
            }
        }
    }
    ids.sort();
    fingerprints.sort();
    fingerprints.dedup();
    (ids, fingerprints)
}

fn current_head_verification_passed(root: &Path, current_head: &str) -> bool {
    let path = root.join("dist/verification.json");
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(evidence) =
        serde_json::from_str::<serde_json::Value>(text.trim_start_matches('\u{feff}'))
    else {
        return false;
    };
    evidence["commit"].as_str() == Some(current_head)
        && evidence["all_pass"].as_bool() == Some(true)
}

fn evidence_covers(
    fingerprints: &[String],
    target_fingerprint: Option<&str>,
    has_passed_record: bool,
    current_head_verification: bool,
) -> bool {
    if !has_passed_record {
        return false;
    }
    if current_head_verification {
        return true;
    }
    match target_fingerprint {
        Some(target) => fingerprints
            .iter()
            .any(|record| crate::git::fingerprint_endorses(record, target)),
        None => true,
    }
}

fn reconcile_entry(
    root: &Path,
    kind: &str,
    entry: &Entry,
    current_head: &str,
    current_worktree_fingerprint: Option<&str>,
) -> ReconcileItem {
    let (has_ledger, ledger_paths, ledger_commit, ledger_test_record_ids) =
        ledger_scope(root, &entry.id);
    let declared_commit = ledger_commit.or_else(|| declared_commit(entry));
    let declared_source_fingerprint = declared_fingerprint(entry);
    let (passed_test_record_ids, evidence_source_fingerprints) = passed_evidence(root, &entry.id);
    let mut test_record_ids = passed_test_record_ids.clone();
    for record_id in ledger_test_record_ids {
        if !test_record_ids.contains(&record_id) {
            test_record_ids.push(record_id);
        }
    }
    let source_files = if has_ledger {
        ledger_paths
    } else {
        declared_commit
            .as_deref()
            .filter(|commit| is_ancestor(root, commit, current_head))
            .map(|commit| changed_source_files(root, commit, current_head))
            .unwrap_or_default()
    };
    let dirty_paths = current_worktree_fingerprint
        .filter(|_| has_ledger)
        .map(|_| changed_worktree_source_files(root, &source_files))
        .unwrap_or_default();
    let current_entry_fingerprint = if !has_ledger {
        // 遗留条目没有可关联的账本 paths，保留旧的全树脏提示；只读提示不参与硬门禁。
        current_worktree_fingerprint.map(str::to_string)
    } else if dirty_paths.is_empty() {
        None
    } else {
        crate::git::source_endorsement_fingerprint_for_paths(root, &dirty_paths)
            .ok()
            .filter(|value| !value.is_empty())
    };
    let historical_fingerprint = head_fingerprint_for_paths(root, &source_files);
    let target_fingerprint = current_entry_fingerprint
        .as_deref()
        .or(historical_fingerprint.as_deref());
    let mut reasons = Vec::new();

    let classification = match declared_commit.as_deref() {
        None => {
            reasons.push("缺少可验证的声明 commit/observed_head".into());
            ReconcileClass::Stale
        }
        Some(commit) if !is_ancestor(root, commit, current_head) => {
            reasons.push(format!(
                "声明 commit `{commit}` 不在当前 HEAD `{current_head}` 祖先链"
            ));
            ReconcileClass::Stale
        }
        Some(commit) => {
            if let Some(declared) = declared_source_fingerprint.as_deref() {
                if let Some(current) = current_entry_fingerprint.as_deref() {
                    if !crate::git::fingerprint_endorses(declared, current) {
                        reasons.push("声明的源码指纹未覆盖当前工作树源码改动".into());
                    }
                }
            }
            if let Some(current) = current_entry_fingerprint.as_deref() {
                reasons.push(format!(
                    "当前工作树仍有本条目改动面源码改动，源码指纹 `{current}` 未形成提交"
                ));
                ReconcileClass::ImplementedUncommitted
            } else if !evidence_covers(
                &evidence_source_fingerprints,
                target_fingerprint,
                !passed_test_record_ids.is_empty(),
                current_head_verification_passed(root, current_head),
            ) {
                if let Some(target) = target_fingerprint {
                    reasons.push(format!("测试证据未覆盖源码指纹 `{target}`"));
                } else {
                    reasons.push("没有关联的 passed test_record 证据".into());
                }
                ReconcileClass::CommittedUnverified
            } else {
                reasons.push(format!(
                    "声明 commit `{commit}` 已在当前 HEAD 祖先链，测试证据已关联"
                ));
                ReconcileClass::VerifiedUnclosed
            }
        }
    };

    ReconcileItem {
        id: entry.id.clone(),
        kind: kind.into(),
        title: entry.title.clone(),
        classification,
        reasons,
        declared_commit,
        current_head: current_head.into(),
        declared_source_fingerprint,
        evidence_source_fingerprints,
        test_record_ids,
        source_files,
    }
}

pub fn reconcile_active(
    root: &Path,
    requirements: &[Entry],
    requirement_terminal: &[&str],
    defects: &[Entry],
    defect_terminal: &[&str],
    observation: &RepoObservation,
) -> ReconciliationReport {
    let current_head = current_head(root).unwrap_or_else(|| observation.observed_head.clone());
    let current_worktree = crate::git::source_endorsement_fingerprint(root)
        .ok()
        .filter(|value| !value.is_empty());
    let mut items = Vec::new();
    for (kind, entries, terminal) in [
        ("requirement", requirements, requirement_terminal),
        ("defect", defects, defect_terminal),
    ] {
        for entry in entries {
            if terminal.contains(&entry.status.as_str()) {
                continue;
            }
            let (has_ledger, _, _, _) = ledger_scope(root, &entry.id);
            let item = reconcile_entry(
                root,
                kind,
                entry,
                &current_head,
                current_worktree.as_deref(),
            );
            if has_ledger {
                let legacy_paths = declared_commit(entry)
                    .as_deref()
                    .filter(|commit| is_ancestor(root, commit, &current_head))
                    .map(|commit| changed_source_files(root, commit, &current_head))
                    .unwrap_or_default();
                if legacy_paths != item.source_files {
                    let ctx = kanzei_harness::ToolCtx::new(root.to_path_buf(), root.to_path_buf());
                    crate::work::log::append(
                        root,
                        "reconcile_observation",
                        Some(&entry.id),
                        &ctx,
                        serde_json::json!({
                            "old_paths": legacy_paths,
                            "new_paths": item.source_files,
                            "decision": "unchanged",
                            "source": "engine",
                        }),
                    );
                }
            }
            items.push(item);
        }
    }
    items.sort_by(|left, right| left.id.cmp(&right.id));
    let mut counts = BTreeMap::new();
    for item in &items {
        *counts
            .entry(classification_name(item.classification).to_string())
            .or_insert(0) += 1;
    }
    ReconciliationReport { items, counts }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification_names_are_stable_machine_values() {
        assert_eq!(classification_name(ReconcileClass::Stale), "stale");
        assert_eq!(
            classification_name(ReconcileClass::ImplementedUncommitted),
            "implemented-uncommitted"
        );
        assert_eq!(
            classification_name(ReconcileClass::CommittedUnverified),
            "committed-unverified"
        );
        assert_eq!(
            classification_name(ReconcileClass::VerifiedUnclosed),
            "verified-unclosed"
        );
    }

    #[test]
    fn fingerprint_evidence_requires_each_current_path() {
        let target =
            "v2 crates/kanzei-app/ui/a.js@111111111111,crates/kanzei-app/ui/b.js@222222222222";
        assert!(evidence_covers(&[target.into()], Some(target), true, false));
        assert!(!evidence_covers(
            &["v2 crates/kanzei-app/ui/a.js@111111111111".into()],
            Some(target),
            true,
            false
        ));
        assert!(!evidence_covers(
            &[target.into()],
            Some(target),
            false,
            false
        ));
    }

    #[test]
    fn current_head_verify_covers_historical_source_paths() {
        let root = git_fixture("current-head-verify");
        let base = head(&root);
        std::fs::write(
            root.join("crates/example/src/lib.rs"),
            "pub fn value() -> u8 { 2 }\n",
        )
        .unwrap();
        let run = |args: &[&str]| {
            let status = Command::new("git")
                .current_dir(&root)
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?} failed");
        };
        run(&["add", "."]);
        run(&["commit", "--quiet", "-m", "feature"]);
        let current = head(&root);
        std::fs::create_dir_all(root.join("dist")).unwrap();
        std::fs::write(
            root.join("dist/verification.json"),
            format!(r#"{{"commit":"{current}","all_pass":true}}"#),
        )
        .unwrap();
        crate::test_record::append_test_run(
            &root,
            "D-current-head verification",
            "passed",
            Some(".\\scripts\\verify.ps1 -Full"),
            Some("passed"),
            Some(&["D-current-head".to_string()]),
        )
        .unwrap();
        let entry = active("D-current-head", &base);
        let report = reconcile_active(
            &root,
            &[],
            &[],
            &[entry],
            &[],
            &RepoObservation {
                recorded_at: "now".into(),
                observed_head: current.clone(),
                observed_worktree_hash: "clean".into(),
            },
        );
        assert_eq!(
            report.items[0].classification,
            ReconcileClass::VerifiedUnclosed
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn ledger_paths_are_authoritative_and_legacy_divergence_is_observed() {
        let root = git_fixture("ledger-scope");
        let base = head(&root);
        std::fs::write(
            root.join("crates/example/src/lib.rs"),
            "pub fn value() -> u8 { 2 }\n",
        )
        .unwrap();
        std::fs::write(
            root.join("crates/example/src/other.rs"),
            "pub fn unrelated() -> u8 { 9 }\n",
        )
        .unwrap();
        let run = |args: &[&str]| {
            let status = Command::new("git")
                .current_dir(&root)
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?} failed");
        };
        run(&["add", "."]);
        run(&["commit", "--quiet", "-m", "deliver"]);
        let current = head(&root);
        let ctx = kanzei_harness::ToolCtx::new(root.clone(), root.clone());
        crate::work::log::append(
            &root,
            "deliver",
            Some("R-353"),
            &ctx,
            serde_json::json!({
                "commit": current,
                "paths": ["crates/example/src/lib.rs"],
                "test_record_ids": ["T-ledger"]
            }),
        );
        std::fs::create_dir_all(root.join("dist")).unwrap();
        std::fs::write(
            root.join("dist/verification.json"),
            format!(r#"{{"commit":"{current}","all_pass":true}}"#),
        )
        .unwrap();
        crate::test_record::append_test_run(
            &root,
            "R-353 verify",
            "passed",
            Some(".\\scripts\\verify.ps1 -Full"),
            Some("passed"),
            Some(&["R-353".to_string()]),
        )
        .unwrap();
        let report = reconcile_active(
            &root,
            &[],
            &[],
            &[active("R-353", &base)],
            &[],
            &RepoObservation {
                recorded_at: "now".into(),
                observed_head: current,
                observed_worktree_hash: "clean".into(),
            },
        );
        let item = &report.items[0];
        assert_eq!(item.classification, ReconcileClass::VerifiedUnclosed);
        assert_eq!(item.source_files, ["crates/example/src/lib.rs"]);
        assert!(item.test_record_ids.iter().any(|id| id != "T-ledger"));
        let log = std::fs::read_to_string(root.join(crate::work::log::WORK_LOG_REL)).unwrap();
        assert!(log.contains("reconcile_observation"), "{log}");
        assert!(log.contains("old_paths"), "{log}");
        let _ = std::fs::remove_dir_all(root);
    }

    fn git_fixture(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("kz-reconcile-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        let run = |args: &[&str]| {
            let status = Command::new("git")
                .current_dir(&root)
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?} failed");
        };
        run(&["init", "--quiet"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Kanzei Test"]);
        std::fs::create_dir_all(root.join("crates/example/src")).unwrap();
        std::fs::write(
            root.join("crates/example/src/lib.rs"),
            "pub fn value() -> u8 { 1 }\n",
        )
        .unwrap();
        run(&["add", "."]);
        run(&["commit", "--quiet", "-m", "base"]);
        root
    }

    fn head(root: &std::path::Path) -> String {
        git_stdout(root, &["rev-parse", "HEAD"]).unwrap()
    }

    fn active(id: &str, commit: &str) -> Entry {
        Entry {
            id: id.into(),
            title: format!("{id} title"),
            status: "fixing".into(),
            severity: Some("medium".into()),
            fields: vec![("observed_head".into(), commit.into())],
        }
    }

    #[test]
    fn reconcile_reports_committed_unverified_verified_and_stale() {
        let root = git_fixture("classes");
        let current = head(&root);
        let refs = vec!["D-verified".to_string()];
        crate::test_record::append_test_run(
            &root,
            "D-verified verification",
            "passed",
            Some("cargo test -p example"),
            Some("passed"),
            Some(&refs),
        )
        .unwrap();
        let entries = vec![
            active("D-unverified", &current),
            active("D-verified", &current),
            active("D-stale", "deadbeef"),
        ];
        let report = reconcile_active(
            &root,
            &[],
            &[],
            &entries,
            &[],
            &RepoObservation {
                recorded_at: "now".into(),
                observed_head: current.clone(),
                observed_worktree_hash: "clean".into(),
            },
        );
        let class = |id: &str| {
            report
                .items
                .iter()
                .find(|item| item.id == id)
                .unwrap()
                .classification
        };
        assert_eq!(class("D-unverified"), ReconcileClass::CommittedUnverified);
        assert_eq!(class("D-verified"), ReconcileClass::VerifiedUnclosed);
        assert_eq!(class("D-stale"), ReconcileClass::Stale);
        assert!(report.already_committed("D-unverified"));
        assert!(report.already_committed("D-verified"));
        assert!(!report.already_committed("D-stale"));
        assert_eq!(report.items[0].current_head, current);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reconcile_detects_source_changes_left_uncommitted() {
        let root = git_fixture("uncommitted");
        let current = head(&root);
        std::fs::write(
            root.join("crates/example/src/lib.rs"),
            "pub fn value() -> u8 { 2 }\n",
        )
        .unwrap();
        let entry = active("D-uncommitted", &current);
        let observation = super::super::repo_observation(&root);
        let report = reconcile_active(&root, &[], &[], &[entry], &[], &observation);
        let item = &report.items[0];
        assert_eq!(item.classification, ReconcileClass::ImplementedUncommitted);
        assert!(item.reasons.iter().any(|reason| reason.contains("工作树")));
        assert!(!report.already_committed("D-uncommitted"));
        let _ = std::fs::remove_dir_all(root);
    }
}
