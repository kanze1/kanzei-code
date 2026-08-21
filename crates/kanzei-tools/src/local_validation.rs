//! 写入后的低成本局部结构校验(R-320)。
//!
//! 该模块只负责“写入已完成”之后的诊断计划与结果投影：它不回滚写入，
//! 不把局部通过当成 crate/workspace 回归，也不通过 shell 拼接命令。
//! 同一路径的并发连续写入用短窗口合并，最后一次写入负责实际校验。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde_json::{json, Value};
use similar::{ChangeTag, TextDiff};
use tokio::process::Command;

const DEBOUNCE_WINDOW: Duration = Duration::from_millis(75);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(20);

static DEBOUNCE_GENERATIONS: OnceLock<Mutex<HashMap<PathBuf, u64>>> = OnceLock::new();

#[derive(Debug, Clone)]
pub(crate) struct ValidationReport {
    pub summary: String,
    pub display: Value,
}

#[derive(Debug, Clone)]
struct CheckReport {
    kind: &'static str,
    status: &'static str,
    command: String,
    first_error: Option<String>,
    repair_context: Option<String>,
}

#[derive(Debug, Clone)]
enum ValidationTask {
    Internal {
        kind: &'static str,
        command: &'static str,
        result: Result<(), String>,
    },
    External {
        kind: &'static str,
        program: &'static str,
        args: Vec<String>,
        command: String,
    },
    Unsupported {
        kind: &'static str,
        command: String,
        context: String,
    },
}

/// 在专用写者落盘后执行。`old_content=None` 表示新文件，changed region 覆盖全文。
pub(crate) async fn validate_after_write(
    path: &Path,
    project_root: &Path,
    old_content: Option<&str>,
    new_content: &str,
) -> ValidationReport {
    let changed_region = changed_region(old_content, new_content);
    let debounce_key = path
        .strip_prefix(project_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    let generation = register_generation(path);

    // 75ms 让同一路径的并发连续写入先汇合；只有最新 generation 运行命令。
    tokio::time::sleep(DEBOUNCE_WINDOW).await;
    if !is_latest_generation(path, generation) {
        return report(
            &debounce_key,
            changed_region,
            vec![CheckReport {
                kind: "debounce",
                status: "debounced",
                command: "local validation merged into the latest same-file write".into(),
                first_error: None,
                repair_context: Some(
                    "同一路径的新写入已接管校验；请以最新写入返回的 local_validation 为准。".into(),
                ),
            }],
        );
    }

    let tasks = plan_tasks(path, project_root, new_content);
    let mut checks = Vec::with_capacity(tasks.len());
    for task in tasks {
        checks.push(run_task(task, project_root).await);
    }
    report(&debounce_key, changed_region, checks)
}

fn report(debounce_key: &str, changed_region: Value, checks: Vec<CheckReport>) -> ValidationReport {
    let failed = checks
        .iter()
        .filter(|check| check.status == "failed")
        .count();
    let unsupported = checks
        .iter()
        .filter(|check| check.status == "unsupported")
        .count();
    let debounced = checks
        .iter()
        .filter(|check| check.status == "debounced")
        .count();
    let passed = checks
        .iter()
        .filter(|check| check.status == "passed")
        .count();
    let headline = if failed > 0 {
        format!("局部结构校验发现 {failed} 个精确错误，请先修复后再扩大回归")
    } else if debounced > 0 {
        "局部结构校验已合并到同文件最新写入".into()
    } else if unsupported > 0 {
        format!("局部结构校验: {unsupported} 个校验器 unsupported，仅提供建议命令")
    } else {
        format!("局部结构校验通过: {passed} 个低成本检查")
    };
    let detail = checks
        .iter()
        .map(|check| {
            let mut line = format!(
                "- {} [{}] command: {}",
                check.kind, check.status, check.command
            );
            if let Some(first_error) = &check.first_error {
                line.push_str(&format!("\n  首个错误: {first_error}"));
            }
            if let Some(context) = &check.repair_context {
                line.push_str(&format!("\n  修复上下文: {context}"));
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n");
    let summary = format!("{headline}\n局部校验明细:\n{detail}");
    let checks_json = checks
        .iter()
        .map(|check| {
            json!({
                "kind": check.kind,
                "status": check.status,
                "command": check.command,
                "first_error": check.first_error,
                "repair_context": check.repair_context,
            })
        })
        .collect::<Vec<_>>();
    ValidationReport {
        summary,
        display: json!({
            "kind": "local_validation",
            "changed_region": changed_region,
            "debounce_key": debounce_key,
            "checks": checks_json,
            "counts": {
                "passed": passed,
                "failed": failed,
                "unsupported": unsupported,
                "debounced": debounced,
            },
            "is_crate_regression": false,
        }),
    }
}

fn plan_tasks(path: &Path, project_root: &Path, content: &str) -> Vec<ValidationTask> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut tasks = Vec::new();
    match extension.as_str() {
        "json" => tasks.push(ValidationTask::Internal {
            kind: "json_parser",
            command: "serde_json::from_str",
            result: serde_json::from_str::<Value>(content)
                .map(|_| ())
                .map_err(|error| error.to_string()),
        }),
        "toml" => tasks.push(ValidationTask::Internal {
            kind: "toml_parser",
            command: "toml::Table::from_str",
            result: content
                .parse::<toml::Table>()
                .map(|_| ())
                .map_err(|error| error.to_string()),
        }),
        "js" | "mjs" | "cjs" => {
            let path_text = path.display().to_string();
            tasks.push(ValidationTask::External {
                kind: "javascript_syntax",
                program: "node",
                args: vec!["--check".into(), path_text.clone()],
                command: format!("node --check \"{path_text}\""),
            });
            let relative = relative_path(path, project_root);
            if is_ui_path(&relative) || relative == "scripts/ui-lint-smoke.mjs" {
                tasks.push(ValidationTask::External {
                    kind: "ui_lint_probe",
                    program: "node",
                    args: vec!["scripts/ui-lint-smoke.mjs".into()],
                    command: "node scripts/ui-lint-smoke.mjs".into(),
                });
            }
            if is_vm_probe_path(&relative) {
                tasks.push(ValidationTask::External {
                    kind: "vm_runtime_probe",
                    program: "node",
                    args: vec!["scripts/ui-runtime-smoke.mjs".into()],
                    command: "node scripts/ui-runtime-smoke.mjs".into(),
                });
            }
        }
        "rs" => {
            let path_text = path.display().to_string();
            tasks.push(ValidationTask::External {
                kind: "rust_ast_formatter",
                program: "rustfmt",
                args: vec!["--check".into(), path_text.clone()],
                command: format!("rustfmt --check \"{path_text}\""),
            });
            if let Some(manifest) = nearest_manifest(path, project_root) {
                let manifest_text = manifest.display().to_string();
                tasks.push(ValidationTask::External {
                    kind: "rust_target_check",
                    program: "cargo",
                    args: vec![
                        "check".into(),
                        "--manifest-path".into(),
                        manifest_text.clone(),
                    ],
                    command: format!("cargo check --manifest-path \"{manifest_text}\""),
                });
            } else {
                tasks.push(ValidationTask::Unsupported {
                    kind: "rust_target_check",
                    command: "cargo check --manifest-path <nearest Cargo.toml>".into(),
                    context: "未找到所属 Cargo.toml；建议在目标 crate 根目录执行 cargo check。".into(),
                });
            }
        }
        _ => tasks.push(ValidationTask::Unsupported {
            kind: "file_type",
            command: format!("<no reliable validator for .{extension}>") ,
            context: "未知语言或仓内没有可靠局部校验器；可按文件类型手动选择 parser、formatter 或目标 smoke。".into(),
        }),
    }
    tasks
}

async fn run_task(task: ValidationTask, project_root: &Path) -> CheckReport {
    match task {
        ValidationTask::Internal {
            kind,
            command,
            result,
        } => CheckReport {
            kind,
            status: if result.is_ok() { "passed" } else { "failed" },
            command: command.into(),
            first_error: result.as_ref().err().cloned(),
            repair_context: result.err(),
        },
        ValidationTask::Unsupported {
            kind,
            command,
            context,
        } => CheckReport {
            kind,
            status: "unsupported",
            command,
            first_error: None,
            repair_context: Some(context),
        },
        ValidationTask::External {
            kind,
            program,
            args,
            command,
        } => run_external(kind, program, &args, &command, project_root).await,
    }
}

async fn run_external(
    kind: &'static str,
    program: &'static str,
    args: &[String],
    command: &str,
    project_root: &Path,
) -> CheckReport {
    let mut child = Command::new(program);
    child.args(args).current_dir(project_root);
    crate::hide_console_async(&mut child);
    let output = match tokio::time::timeout(COMMAND_TIMEOUT, child.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            return CheckReport {
                kind,
                status: "unsupported",
                command: command.into(),
                first_error: None,
                repair_context: Some(format!(
                    "校验器 `{program}` 不在 PATH；安装它或手动执行建议命令: {command}"
                )),
            }
        }
        Ok(Err(error)) => {
            return CheckReport {
                kind,
                status: "failed",
                command: command.into(),
                first_error: Some(error.to_string()),
                repair_context: Some(format!("启动校验器失败: {error}")),
            }
        }
        Err(_) => {
            return CheckReport {
                kind,
                status: "failed",
                command: command.into(),
                first_error: Some(format!("命令超过 {:?} 超时", COMMAND_TIMEOUT)),
                repair_context: Some(format!("缩小改动范围后重试: {command}")),
            }
        }
    };
    if output.status.success() {
        return CheckReport {
            kind,
            status: "passed",
            command: command.into(),
            first_error: None,
            repair_context: None,
        };
    }
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    let diagnostic = if diagnostic.trim().is_empty() {
        String::from_utf8_lossy(&output.stdout)
    } else {
        diagnostic
    };
    let lines = diagnostic
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::trim)
        .collect::<Vec<_>>();
    let first_error = lines.first().map(|line| (*line).to_string());
    let repair_context = if lines.is_empty() {
        Some(format!("命令以 {:?} 退出: {command}", output.status.code()))
    } else {
        Some(lines.iter().take(8).copied().collect::<Vec<_>>().join("\n"))
    };
    CheckReport {
        kind,
        status: "failed",
        command: command.into(),
        first_error,
        repair_context,
    }
}

fn register_generation(path: &Path) -> u64 {
    let state = DEBOUNCE_GENERATIONS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut state = state.lock().unwrap();
    let generation = state.entry(path.to_path_buf()).or_insert(0);
    *generation += 1;
    *generation
}

fn is_latest_generation(path: &Path, generation: u64) -> bool {
    DEBOUNCE_GENERATIONS
        .get()
        .and_then(|state| state.lock().ok())
        .and_then(|state| state.get(path).copied())
        == Some(generation)
}

fn relative_path(path: &Path, project_root: &Path) -> String {
    path.strip_prefix(project_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn is_ui_path(relative: &str) -> bool {
    relative.starts_with("crates/kanzei-app/ui/")
        || relative.starts_with("crates/kanzei-app/mobile-pwa/")
}

fn is_vm_probe_path(relative: &str) -> bool {
    is_ui_path(relative) || relative == "scripts/ui-runtime-smoke.mjs"
}

fn nearest_manifest(path: &Path, project_root: &Path) -> Option<PathBuf> {
    let mut current = path.parent()?;
    loop {
        let candidate = current.join("Cargo.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
        if current == project_root || !current.starts_with(project_root) {
            return None;
        }
        current = current.parent()?;
    }
}

fn changed_region(old_content: Option<&str>, new_content: &str) -> Value {
    let Some(old_content) = old_content else {
        return json!({
            "start_line": 1,
            "end_line": new_content.lines().count().max(1),
            "scope": "whole_file",
        });
    };
    let diff = TextDiff::from_lines(old_content, new_content);
    let mut old_line = 1usize;
    let mut new_line = 1usize;
    let mut start = usize::MAX;
    let mut end = 0usize;
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Delete => {
                start = start.min(old_line);
                end = end.max(old_line);
                old_line += 1;
            }
            ChangeTag::Insert => {
                start = start.min(new_line);
                end = end.max(new_line);
                new_line += 1;
            }
            ChangeTag::Equal => {
                old_line += 1;
                new_line += 1;
            }
        }
    }
    if start == usize::MAX {
        start = 1;
        end = 1;
    }
    json!({
        "start_line": start,
        "end_line": end.max(start),
        "scope": "changed_lines",
    })
}

#[cfg(test)]
mod tests {
    use super::{changed_region, plan_tasks, relative_path, ValidationTask};
    use serde_json::json;
    use std::path::Path;

    #[test]
    fn changed_region_reports_replaced_lines() {
        assert_eq!(
            changed_region(Some("a\nb\nc\n"), "a\nchanged\nc\n"),
            json!({"start_line": 2, "end_line": 2, "scope": "changed_lines"})
        );
    }

    #[test]
    fn new_file_region_covers_written_content() {
        assert_eq!(
            changed_region(None, "a\nb\n"),
            json!({"start_line": 1, "end_line": 2, "scope": "whole_file"})
        );
    }

    #[test]
    fn javascript_plan_contains_syntax_and_vm_probes_for_ui() {
        let root = Path::new("C:/project");
        let path = root.join("crates/kanzei-app/ui/main.js");
        let tasks = plan_tasks(&path, root, "const x = 1;");
        assert!(tasks.iter().any(|task| matches!(
            task,
            ValidationTask::External {
                kind: "javascript_syntax",
                ..
            }
        )));
        assert!(tasks.iter().any(|task| matches!(
            task,
            ValidationTask::External {
                kind: "vm_runtime_probe",
                ..
            }
        )));
    }

    #[test]
    fn rust_plan_contains_formatter_and_target_check() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let path = root.join("src/edit.rs");
        let tasks = plan_tasks(&path, root, "const x = 1;");
        assert!(tasks.iter().any(|task| matches!(
            task,
            ValidationTask::External {
                kind: "rust_ast_formatter",
                ..
            }
        )));
        assert!(tasks.iter().any(|task| matches!(
            task,
            ValidationTask::External {
                kind: "rust_target_check",
                ..
            }
        )));
    }

    #[test]
    fn unknown_extension_is_explicitly_unsupported() {
        let tasks = plan_tasks(
            Path::new("C:/project/example.vm"),
            Path::new("C:/project"),
            "fixture",
        );
        assert!(tasks.iter().any(|task| matches!(
            task,
            ValidationTask::Unsupported {
                kind: "file_type",
                ..
            }
        )));
    }

    fn temp_path(label: &str, extension: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "kz-local-validation-{label}-{}.{}",
            std::process::id(),
            extension
        ))
    }

    #[tokio::test]
    async fn javascript_probe_returns_first_error_location() {
        let path = temp_path("syntax", "js");
        std::fs::write(&path, "function broken( {\n").unwrap();
        let report = super::validate_after_write(
            &path,
            Path::new(env!("CARGO_MANIFEST_DIR")),
            None,
            "function broken( {\n",
        )
        .await;
        let check = &report.display["checks"][0];
        assert_eq!(check["status"], "failed");
        assert!(check["first_error"].as_str().is_some());
        assert!(report
            .summary
            .contains(check["first_error"].as_str().unwrap()));
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn json_parser_reports_invalid_written_content() {
        let path = temp_path("json", "json");
        let content = "{broken";
        std::fs::write(&path, content).unwrap();
        let report = super::validate_after_write(
            &path,
            Path::new(env!("CARGO_MANIFEST_DIR")),
            None,
            content,
        )
        .await;
        let check = &report.display["checks"][0];
        assert_eq!(check["kind"], "json_parser");
        assert_eq!(check["status"], "failed");
        assert!(check["first_error"].as_str().unwrap().contains("line"));
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn rustfmt_probe_reports_ast_error() {
        let path = temp_path("rust", "rs");
        let content = "fn broken( {\n";
        std::fs::write(&path, content).unwrap();
        let report = super::validate_after_write(
            &path,
            Path::new(env!("CARGO_MANIFEST_DIR")),
            None,
            content,
        )
        .await;
        let check = &report.display["checks"][0];
        assert_eq!(check["kind"], "rust_ast_formatter");
        assert_eq!(check["status"], "failed");
        assert!(check["first_error"].as_str().is_some());
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn concurrent_same_file_writes_coalesce_to_latest_generation() {
        let path = temp_path("debounce", "vm");
        let root = std::env::temp_dir();
        let (first, second) = tokio::join!(
            super::validate_after_write(&path, &root, None, "first"),
            super::validate_after_write(&path, &root, None, "second")
        );
        let first_status = first.display["checks"][0]["status"].as_str().unwrap();
        let second_status = second.display["checks"][0]["status"].as_str().unwrap();
        assert!(
            (first_status == "debounced" && second_status == "unsupported")
                || (first_status == "unsupported" && second_status == "debounced")
        );
    }

    #[test]
    fn relative_path_uses_forward_slashes() {
        assert_eq!(
            relative_path(
                Path::new("C:/project/crates/kanzei-app/ui/main.js"),
                Path::new("C:/project")
            ),
            "crates/kanzei-app/ui/main.js"
        );
    }
}
