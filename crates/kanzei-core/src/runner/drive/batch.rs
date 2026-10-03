//! 长运行的交付窗口。这里只限制本轮实现阶段，不建立第二套完成批数账本。
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use kanzei_harness::ToolCtx;
use kanzei_llm::Part;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

type Call = (String, String, Value, String);
const IMPLEMENT_STEPS: u32 = 32;
const CLOSE_STEPS: u32 = 8;
const IMPLEMENT_TIME: Duration = Duration::from_secs(15 * 60);
const FILE_LIMIT: u64 = 4 * 1024 * 1024;

fn hide_console(command: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    #[cfg(not(windows))]
    let _ = command;
}

struct Source {
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
    owned: bool,
}

pub(super) struct Batch {
    enabled: bool,
    start_step: u32,
    started: Instant,
    closing: Option<u32>,
    files: BTreeMap<String, Source>,
    receipts: Vec<Value>,
    committed: bool,
    progress_recorded: bool,
    validated: bool,
    shell_changes: bool,
    error: Option<String>,
}

fn read_small(path: &Path) -> Option<Vec<u8>> {
    (std::fs::metadata(path).ok()?.len() <= FILE_LIMIT)
        .then(|| std::fs::read(path).ok())
        .flatten()
}

fn write_target(name: &str, input: &Value, ctx: &ToolCtx) -> Option<(String, PathBuf)> {
    let relative = match name {
        "edit" | "insert" | "write" => input["path"].as_str().map(str::to_owned),
        "req" | "defect"
            if matches!(input["action"].as_str(), Some("add" | "update" | "close"))
                && ctx.cwd == ctx.project_root =>
        {
            Some(format!(
                ".kanzei/project/{}.md",
                if name == "req" {
                    "requirements"
                } else {
                    "defects"
                }
            ))
        }
        _ => None,
    }?;
    let path = ctx.cwd.join(&relative);
    let path = kanzei_base::path_form::canonical_or_simplified(&path);
    let root = kanzei_base::path_form::canonical_or_simplified(&ctx.cwd);
    let Ok(relative) = path.strip_prefix(root) else {
        return None;
    };
    if relative
        .components()
        .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return None;
    }
    let key = relative.to_string_lossy().replace('\\', "/");
    Some((key, path))
}

impl Batch {
    pub(super) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            start_step: 1,
            started: Instant::now(),
            closing: None,
            files: BTreeMap::new(),
            receipts: Vec::new(),
            committed: false,
            progress_recorded: false,
            validated: false,
            shell_changes: false,
            error: None,
        }
    }

    pub(super) fn is_closing(&self) -> bool {
        self.closing.is_some()
    }

    /// 只捕获实际将执行的专用写入。HEAD 不一致、过大文件和 shell 改动均不作为可暂存归属。
    pub(super) fn before_calls(&mut self, calls: &[Call], ctx: &ToolCtx) {
        if !self.enabled {
            return;
        }
        for (_, name, input, _) in calls {
            if self.reject(name, input, ctx).is_some() {
                continue;
            }
            if name == "bash" && !verification_command(input["command"].as_str().unwrap_or("")) {
                self.shell_changes = true;
            }
            let Some((key, path)) = write_target(name, input, ctx) else {
                continue;
            };
            if let Some(source) = self.files.get_mut(&key) {
                if source.after != read_small(&path) {
                    source.owned = false;
                }
                continue;
            }
            let before = read_small(&path);
            let mut command = std::process::Command::new("git");
            command
                .args([
                    "status",
                    "--porcelain=v1",
                    "--untracked-files=all",
                    "--",
                    &key,
                ])
                .current_dir(&ctx.cwd);
            hide_console(&mut command);
            let head = command.output().ok();
            let owned = match head {
                Some(output) if output.status.success() => {
                    output.stdout.is_empty() && (before.is_some() || !path.exists())
                }
                Some(_) => false,
                None => false,
            };
            self.files.insert(
                key,
                Source {
                    before: before.clone(),
                    after: before,
                    owned,
                },
            );
        }
    }

    pub(super) fn observe(&mut self, calls: &[Call], results: &[Part], ctx: &ToolCtx) {
        if !self.enabled {
            return;
        }
        let written = calls
            .iter()
            .zip(results)
            .filter_map(|((_, name, input, _), result)| {
                matches!(
                    result,
                    Part::ToolResult {
                        is_error: false,
                        ..
                    }
                )
                .then(|| write_target(name, input, ctx).map(|(key, _)| key))
                .flatten()
            })
            .collect::<std::collections::BTreeSet<_>>();
        for (relative, source) in &mut self.files {
            let after = read_small(&ctx.cwd.join(relative));
            if !relative.starts_with(".kanzei/project/") && source.after != after {
                self.committed = false;
                self.validated = false;
                self.progress_recorded = false;
            }
            if source.after != after && !written.contains(relative) {
                source.owned = false;
            }
            source.after = after;
        }
        for (call, result) in calls.iter().zip(results) {
            let Part::ToolResult {
                is_error, content, ..
            } = result
            else {
                continue;
            };
            let (_, name, input, _) = call;
            let action = input["action"].as_str().unwrap_or("");
            self.receipts
                .push(json!({"tool":name,"input":input,"ok":!is_error,"result":content}));
            if *is_error {
                continue;
            }
            if name == "git" && matches!(action, "commit" | "finalize") {
                self.committed = true;
                self.validated |= action == "finalize";
            }
            if matches!(name.as_str(), "req" | "defect") && matches!(action, "update" | "close") {
                self.progress_recorded = true;
            }
            if name == "test_record" {
                let parsed: Value = serde_json::from_str(content).unwrap_or(Value::Null);
                self.validated |= input["status"] == "passed" && parsed["status"] != "failed";
            }
        }
    }

    pub(super) fn prepare(&mut self, step: u32, ctx: &ToolCtx) -> Option<String> {
        self.prepare_inner(step, ctx, false)
    }

    pub(super) fn finish_and_resume(&mut self, step: u32, ctx: &ToolCtx) -> Option<String> {
        self.prepare_inner(step, ctx, true)
    }

    fn prepare_inner(&mut self, step: u32, ctx: &ToolCtx, force: bool) -> Option<String> {
        if !self.enabled || self.files.is_empty() {
            return None;
        }
        if self.closing.is_none()
            && (step.saturating_sub(self.start_step) >= IMPLEMENT_STEPS
                || self.started.elapsed() >= IMPLEMENT_TIME)
        {
            self.closing = Some(step);
        }
        let since = self.closing?;
        if (self.committed && self.progress_recorded && self.validated)
            || step.saturating_sub(since) >= CLOSE_STEPS
            || force
        {
            match self.save_checkpoint(ctx, step) {
                Ok(path) => {
                    let delivered = self.committed && self.progress_recorded && self.validated;
                    let remaining = if delivered {
                        BTreeMap::new()
                    } else {
                        std::mem::take(&mut self.files)
                    };
                    *self = Self::new(true);
                    self.files = remaining;
                    self.start_step = step;
                    return Some(format!("(system) Batch checkpoint saved at {}. Delivery recorded: {delivered}. Continue the delegated work from the recorded next action. A checkpoint is not a completed batch or acceptance evidence; completed batch counts remain derived from Git. Do not ask the user merely because this window ended.", path.display()));
                }
                Err(error) => self.error = Some(error.to_string()),
            }
        }
        Some(format!("(system) Close this implementation batch now. Do not expand implementation. Run the relevant validation; if it passes and ownership is clear, use structured git stage/commit or finalize for ONLY this batch, then record the actual progress and next action in the existing requirement/defect. Preserve failed checks and mixed ownership as an incomplete checkpoint; do not claim completion. No Work Unit or handwritten completed-batch counter is required. Continue automatically after the checkpoint.{}", self.error.as_ref().map(|e| format!(" Checkpoint save failed: {e}; keep closing and retry, do not lose the changes.")).unwrap_or_default()))
    }

    pub(super) fn reject(&self, name: &str, input: &Value, ctx: &ToolCtx) -> Option<String> {
        if !self.is_closing() {
            return None;
        }
        let blocked = match name {
            "edit" | "insert" | "write" | "task" | "question" => true,
            "bash" => !verification_command(input["command"].as_str().unwrap_or("")),
            "git" if matches!(input["action"].as_str(), Some("stage" | "finalize")) => {
                input["files"].as_array().is_none_or(|files| {
                    files.is_empty()
                        || files.iter().any(|f| {
                            let Some(path) = f.as_str() else {
                                return true;
                            };
                            let Some(source) = self.files.get(&path.replace('\\', "/")) else {
                                return true;
                            };
                            !source.owned || source.after != read_small(&ctx.cwd.join(path))
                        })
                })
            }
            "git" => !matches!(
                input["action"].as_str(),
                Some("status" | "diff" | "log" | "commit_plan" | "preflight" | "commit")
            ),
            "req" | "defect" => input["action"] == "add",
            "work" => matches!(input["action"].as_str(), Some("claim" | "create_unit")),
            _ => false,
        };
        blocked.then(|| "BATCH_CLOSING: implementation is paused for this delivery window. Run validation and record real progress; stage only attributed, unchanged batch files. Mixed/preexisting edits must stay in the recovery checkpoint. This does not end the user's task.".into())
    }

    fn save_checkpoint(&self, ctx: &ToolCtx, step: u32) -> std::io::Result<PathBuf> {
        let run = ctx.run_id.as_deref().unwrap_or("anonymous");
        let key = format!(
            "{:x}",
            Sha256::digest(format!("{run}:{:?}:{step}", self.started).as_bytes())
        );
        let directory = ctx
            .project_root
            .join(".kanzei/artifacts/batch-checkpoints")
            .join(key);
        std::fs::create_dir_all(&directory)?;
        let mut files = Vec::new();
        for (index, (path, source)) in self.files.iter().enumerate() {
            let before = format!("{index}-before.bin");
            let after = format!("{index}-after.bin");
            if let Some(bytes) = &source.before {
                std::fs::write(directory.join(&before), bytes)?;
            }
            if let Some(bytes) = &source.after {
                std::fs::write(directory.join(&after), bytes)?;
            }
            files.push(json!({"path":path,"owned":source.owned,"before":source.before.as_ref().map(|_|before),"after":source.after.as_ref().map(|_|after),"capture_limit_bytes":FILE_LIMIT}));
        }
        let mut command = std::process::Command::new("git");
        command
            .args(["diff", "--binary", "HEAD", "--"])
            .args(self.files.keys())
            .current_dir(&ctx.cwd);
        hide_console(&mut command);
        let patch = command.output()?;
        let patch_error = if patch.status.success() {
            std::fs::write(directory.join("working.patch"), patch.stdout)?;
            None
        } else {
            Some(String::from_utf8_lossy(&patch.stderr).into_owned())
        };
        let last_progress = self
            .receipts
            .iter()
            .rev()
            .find(|r| r["ok"] == true && matches!(r["tool"].as_str(), Some("req" | "defect")))
            .map(|r| r["input"].clone());
        let last_failure = self
            .receipts
            .iter()
            .rev()
            .find(|r| r["ok"] == false)
            .cloned();
        let manifest = json!({"kind":"runtime_batch_checkpoint","run_id":run,"step":step,"completed":false,"committed":self.committed,"validation_recorded":self.validated,"progress_recorded":self.progress_recorded,"unattributed_shell_calls":self.shell_changes,"files":files,"receipts":self.receipts,"last_progress":last_progress,"last_failure":last_failure,"patch_error":patch_error,"next_action":"Review last_failure and last_progress above, rerun the failed command or continue the recorded tracker next step. Resume the existing item, not a new unit. A patch against HEAD may include preexisting edits; the before/after captures record this delivery window."});
        let path = directory.join("checkpoint.json");
        kanzei_base::atomic_file::write_atomic(&path, &serde_json::to_string_pretty(&manifest)?)?;
        Ok(path)
    }
}

fn verification_command(command: &str) -> bool {
    let command = command.trim();
    if command.contains([';', '&', '|', '`', '$', '\n', '\r']) {
        return false;
    }
    if let Some(script) = command
        .strip_prefix("node scripts/")
        .or_else(|| command.strip_prefix("node --experimental-vm-modules scripts/"))
    {
        return !script.is_empty() && !script.starts_with([' ', '-']);
    }
    [
        "cargo test",
        "cargo check",
        "cargo clippy",
        "rustfmt --check",
        "rustfmt --edition",
        "node --check",
        "git status",
        "git diff",
        "git log",
    ]
    .iter()
    .any(|prefix| {
        command == *prefix
            || command
                .strip_prefix(prefix)
                .is_some_and(|tail| tail.starts_with(' '))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn d773_closing_allows_deferred_ui_checks_and_rejects_shell_expansion() {
        assert!(verification_command("node scripts/ui-lint-smoke.mjs"));
        assert!(verification_command(
            "node --experimental-vm-modules scripts/ui-runtime-smoke.mjs"
        ));
        assert!(!verification_command("node scripts/"));
        assert!(!verification_command("node scripts/ --eval rewrite()"));
        assert!(!verification_command(
            "node scripts/check.mjs && python rewrite.py"
        ));
        assert!(!verification_command("cargo testfake"));
    }
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);

    struct Repo(PathBuf);
    impl Repo {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "kz-d773-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            let repo = Self(path);
            for args in [
                vec!["init", "-q"],
                vec!["config", "user.name", "kanzei"],
                vec!["config", "user.email", "vraniumzwt@gmail.com"],
            ] {
                repo.git(&args);
            }
            std::fs::write(repo.0.join("own.rs"), "fn base() {}\n").unwrap();
            std::fs::write(repo.0.join("mixed.rs"), "fn old() {}\n").unwrap();
            repo.git(&["add", "own.rs", "mixed.rs"]);
            repo.git(&["commit", "-q", "-m", "base"]);
            repo
        }
        fn git(&self, args: &[&str]) {
            let mut command = std::process::Command::new("git");
            command.args(args).current_dir(&self.0);
            hide_console(&mut command);
            let result = command.output().unwrap();
            assert!(result.status.success(), "{:?}", result);
        }
        fn ctx(&self) -> ToolCtx {
            ToolCtx::new(self.0.clone(), self.0.clone())
        }
    }
    impl Drop for Repo {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn call(name: &str, input: Value) -> Call {
        (format!("{name}-call"), name.into(), input, String::new())
    }
    fn result(ok: bool, content: &str) -> Part {
        Part::ToolResult {
            call_id: "fixture".into(),
            content: content.into(),
            is_error: !ok,
        }
    }
    fn edited(repo: &Repo, batch: &mut Batch, path: &str) {
        let calls = [call("edit", json!({"path":path}))];
        batch.before_calls(&calls, &repo.ctx());
        std::fs::write(repo.0.join(path), "fn changed() {}\n").unwrap();
        batch.observe(&calls, &[result(true, "written")], &repo.ctx());
    }

    #[test]
    fn unrelated_observation_cannot_absorb_external_file_changes() {
        let repo = Repo::new();
        let mut batch = Batch::new(true);
        edited(&repo, &mut batch, "own.rs");
        let read = [call("read", json!({"path":"mixed.rs"}))];
        batch.before_calls(&read, &repo.ctx());
        std::fs::write(repo.0.join("own.rs"), "external edit\n").unwrap();
        batch.observe(&read, &[result(true, "read")], &repo.ctx());
        assert!(!batch.files["own.rs"].owned);
        batch.prepare(33, &repo.ctx());
        for action in ["stage", "finalize"] {
            assert!(batch
                .reject(
                    "git",
                    &json!({"action":action,"files":["own.rs"]}),
                    &repo.ctx()
                )
                .is_some());
        }
    }

    #[test]
    fn failed_write_cannot_claim_changed_bytes() {
        let repo = Repo::new();
        let mut batch = Batch::new(true);
        let calls = [call("write", json!({"path":"own.rs"}))];
        batch.before_calls(&calls, &repo.ctx());
        std::fs::write(repo.0.join("own.rs"), "changed despite failure\n").unwrap();
        batch.observe(&calls, &[result(false, "failed")], &repo.ctx());
        assert!(!batch.files["own.rs"].owned);
    }

    #[test]
    fn d773_closes_at_threshold_and_blocks_expansion_or_foreign_stage() {
        let repo = Repo::new();
        std::fs::write(repo.0.join("mixed.rs"), "preexisting work\n").unwrap();
        let mut batch = Batch::new(true);
        edited(&repo, &mut batch, "own.rs");
        edited(&repo, &mut batch, "mixed.rs");
        assert!(batch.prepare(32, &repo.ctx()).is_none());
        assert!(batch
            .prepare(33, &repo.ctx())
            .unwrap()
            .contains("Close this implementation batch"));
        for name in ["edit", "write", "task", "question"] {
            assert!(batch.reject(name, &json!({}), &repo.ctx()).is_some());
        }
        assert!(batch
            .reject("bash", &json!({"command":"python rewrite.py"}), &repo.ctx())
            .is_some());
        assert!(batch
            .reject(
                "bash",
                &json!({"command":"cargo test -p fixture"}),
                &repo.ctx()
            )
            .is_none());
        assert!(batch
            .reject(
                "git",
                &json!({"action":"stage","files":["own.rs"]}),
                &repo.ctx()
            )
            .is_none());
        assert!(batch
            .reject(
                "git",
                &json!({"action":"stage","files":["mixed.rs"]}),
                &repo.ctx()
            )
            .is_some());
        std::fs::write(repo.0.join("own.rs"), "new foreign edit\n").unwrap();
        assert!(batch
            .reject(
                "git",
                &json!({"action":"stage","files":["own.rs"]}),
                &repo.ctx()
            )
            .is_some());
    }

    #[test]
    fn d773_failed_validation_saves_real_patch_and_resumes_without_completion() {
        let repo = Repo::new();
        let mut batch = Batch::new(true);
        edited(&repo, &mut batch, "own.rs");
        batch.prepare(33, &repo.ctx());
        batch.observe(
            &[call("bash", json!({"command":"cargo test"}))],
            &[result(false, "real failing assertion")],
            &repo.ctx(),
        );
        let note = batch.prepare(41, &repo.ctx()).unwrap();
        assert!(note.contains("Delivery recorded: false"));
        assert!(!batch.is_closing());
        let parent = repo.0.join(".kanzei/artifacts/batch-checkpoints");
        let directory = std::fs::read_dir(parent)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let manifest: Value =
            serde_json::from_slice(&std::fs::read(directory.join("checkpoint.json")).unwrap())
                .unwrap();
        assert_eq!(manifest["completed"], false);
        assert!(manifest["last_failure"]["result"]
            .as_str()
            .unwrap()
            .contains("real failing assertion"));
        assert!(std::fs::read_to_string(directory.join("working.patch"))
            .unwrap()
            .contains("fn changed"));
        assert_eq!(
            std::fs::read(directory.join("0-after.bin")).unwrap(),
            b"fn changed() {}\n"
        );
    }

    #[test]
    fn d773_verified_commit_and_tracker_progress_resume_and_later_edits_invalidate_proof() {
        let repo = Repo::new();
        let mut batch = Batch::new(true);
        edited(&repo, &mut batch, "own.rs");
        batch.prepare(33, &repo.ctx());
        repo.git(&["add", "own.rs"]);
        repo.git(&["commit", "-q", "-m", "D-773 batch fixture"]);
        let calls = [
            call("test_record", json!({"status":"passed"})),
            call("git", json!({"action":"commit"})),
            call(
                "req",
                json!({"action":"update","id":"R-358","fields":{"进展":"verified batch; next: second file"}}),
            ),
        ];
        batch.observe(
            &calls,
            &[
                result(true, "recorded"),
                result(true, "committed"),
                result(true, "updated"),
            ],
            &repo.ctx(),
        );
        assert!(batch
            .prepare(34, &repo.ctx())
            .unwrap()
            .contains("Delivery recorded: true"));
        assert!(!batch.is_closing());
        edited(&repo, &mut batch, "own.rs");
        std::fs::write(repo.0.join("own.rs"), "new unverified code\n").unwrap();
        batch.observe(&[], &[], &repo.ctx());
        assert!(!batch.validated && !batch.committed);
    }

    #[test]
    fn d773_checkpoint_io_failure_keeps_closing_and_does_not_forget_work() {
        let repo = Repo::new();
        let mut batch = Batch::new(true);
        edited(&repo, &mut batch, "own.rs");
        batch.prepare(33, &repo.ctx());
        std::fs::create_dir_all(repo.0.join(".kanzei")).unwrap();
        std::fs::write(repo.0.join(".kanzei/artifacts"), "blocks directory").unwrap();
        let note = batch.prepare(41, &repo.ctx()).unwrap();
        assert!(note.contains("Checkpoint save failed"));
        assert!(batch.is_closing());
        assert!(batch.files.contains_key("own.rs"));
    }
}
