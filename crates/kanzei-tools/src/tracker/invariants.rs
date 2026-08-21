//! R-311 批1：设计冻结不变式的机器执行器。
//!
//! 条目字段 `不变式` 使用单行 JSON 数组登记断言：
//! `[{"kind":"grep","path":"...","pattern":"..."},
//! {"kind":"test","package":"kanzei-tools","name":"..."},
//! {"kind":"script","path":"scripts/check.mjs","args":[]}]`。
//! 所有路径都必须是项目根下的相对路径；脚本不经过 shell，避免把断言字段变成
//! 任意命令注入面。close 与 git finalize 共用本模块，失败返回断言序号和详情。

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::docstore::{DocStore, Entry, DEFECTS, REQUIREMENTS};

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum InvariantSpec {
    Grep {
        path: String,
        pattern: String,
    },
    Test {
        name: String,
        #[serde(default)]
        package: Option<String>,
    },
    Script {
        path: String,
        #[serde(default)]
        args: Vec<String>,
    },
}

fn invariant_text(entry: &Entry) -> Option<&str> {
    entry
        .fields
        .iter()
        .find(|(key, _)| key == "不变式")
        .map(|(_, value)| value.trim())
        .filter(|value| !value.is_empty())
}

fn relative_target(root: &Path, raw: &str) -> Result<PathBuf, String> {
    let path = Path::new(raw);
    if raw.trim().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::Prefix(_)
                    | std::path::Component::RootDir
                    | std::path::Component::ParentDir
            )
        })
    {
        return Err(format!("路径必须是项目根下的安全相对路径: `{raw}`"));
    }
    Ok(root.join(path))
}

fn output_tail(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let text = if stderr.trim().is_empty() {
        stdout.into_owned()
    } else if stdout.trim().is_empty() {
        stderr.into_owned()
    } else {
        format!("{stderr}\n{stdout}")
    };
    let text = text.trim();
    if text.len() <= 600 {
        text.to_string()
    } else {
        format!("…{}", &text[text.len() - 600..])
    }
}

fn run_test(cwd: &Path, name: &str, package: Option<&str>) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("test 断言的 name 不能为空".into());
    }
    let mut command = Command::new("cargo");
    command.arg("test");
    if let Some(package) = package.filter(|value| !value.trim().is_empty()) {
        command.args(["-p", package]);
    }
    command.arg(name).args(["--", "--exact"]);
    command.current_dir(cwd);
    crate::hide_console(&mut command);
    let output = command
        .output()
        .map_err(|error| format!("运行 cargo test 失败: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "cargo test 退出码 {:?}: {}",
            output.status.code(),
            output_tail(&output)
        ))
    }
}

fn run_script(root: &Path, cwd: &Path, path: &str, args: &[String]) -> Result<(), String> {
    let target = relative_target(root, path)?;
    if !target.is_file() {
        return Err(format!("脚本不存在: `{path}`"));
    }
    let extension = target
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut command = match extension.as_str() {
        "ps1" => {
            let mut command = Command::new("pwsh");
            command.args(["-NoProfile", "-File"]);
            command.arg(&target);
            command
        }
        "mjs" | "js" => {
            let mut command = Command::new("node");
            command.arg(&target);
            command
        }
        "cmd" | "bat" => {
            let mut command = Command::new("cmd");
            command.args(["/C"]);
            command.arg(&target);
            command
        }
        _ => Command::new(&target),
    };
    command.args(args).current_dir(cwd);
    crate::hide_console(&mut command);
    let output = command
        .output()
        .map_err(|error| format!("运行脚本 `{path}` 失败: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "脚本退出码 {:?}: {}",
            output.status.code(),
            output_tail(&output)
        ))
    }
}

fn run_one(root: &Path, cwd: &Path, spec: &InvariantSpec) -> Result<(), String> {
    match spec {
        InvariantSpec::Grep { path, pattern } => {
            let target = relative_target(root, path)?;
            let text = std::fs::read_to_string(&target)
                .map_err(|error| format!("读取 `{path}` 失败: {error}"))?;
            // grep 断言按行语义执行：^/$ 应能匹配文本中一行的边界，而不是只匹配
            // 整个字符串边界；这也与设计冻结字段里的 grep 模式直觉一致。
            let regex = regex::RegexBuilder::new(pattern)
                .multi_line(true)
                .build()
                .map_err(|error| format!("grep pattern 无效 `{pattern}`: {error}"))?;
            if regex.is_match(&text) {
                Ok(())
            } else {
                Err(format!("grep 未匹配 `{path}` / `{pattern}`"))
            }
        }
        InvariantSpec::Test { name, package } => run_test(cwd, name, package.as_deref()),
        InvariantSpec::Script { path, args } => run_script(root, cwd, path, args),
    }
}

fn describe(spec: &InvariantSpec) -> String {
    match spec {
        InvariantSpec::Grep { path, pattern } => format!("grep `{path}` / `{pattern}`"),
        InvariantSpec::Test { name, package } => package
            .as_deref()
            .map(|package| format!("test `{package}::{name}`"))
            .unwrap_or_else(|| format!("test `{name}`")),
        InvariantSpec::Script { path, args } => {
            format!(
                "script `{path}`{}",
                if args.is_empty() { "" } else { " + args" }
            )
        }
    }
}

/// 执行条目的全部冻结不变式；没有声明时保持向后兼容，不产生任何门禁。
/// 返回错误时包含断言序号、类型和具体失败原因，供 close/finalize 原样点名。
pub(crate) fn check_entry_invariants(root: &Path, cwd: &Path, entry: &Entry) -> Result<(), String> {
    let Some(raw) = invariant_text(entry) else {
        return Ok(());
    };
    let specs: Vec<InvariantSpec> = serde_json::from_str(raw)
        .map_err(|error| format!("不变式字段不是合法 JSON 数组: {error}"))?;
    let mut failures = Vec::new();
    for (index, spec) in specs.iter().enumerate() {
        if let Err(error) = run_one(root, cwd, spec) {
            failures.push(format!("#{} {}: {error}", index + 1, describe(spec)));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "冻结不变式执行失败，拒绝终态迁移:\n{}",
            failures.join("\n")
        ))
    }
}

/// finalize 的真实绑定入口：按 R-/D- id 读取活动条目，找不到时回落归档只读。
pub(crate) fn check_entry_invariants_by_id(
    root: &Path,
    cwd: &Path,
    entry_id: &str,
) -> Result<(), String> {
    let kind = match entry_id.get(..2) {
        Some("R-") => &REQUIREMENTS,
        Some("D-") => &DEFECTS,
        _ => return Err(format!("不变式绑定 id 必须是 R-/D- 条目: `{entry_id}`")),
    };
    let store = DocStore::open(root, kind);
    let active = store.load().map_err(|error| error.to_string())?;
    let entry = active
        .into_iter()
        .find(|entry| entry.id == entry_id)
        .or_else(|| {
            store
                .load_archive()
                .ok()?
                .into_iter()
                .find(|entry| entry.id == entry_id)
        })
        .ok_or_else(|| format!("找不到不变式绑定条目 `{entry_id}`"))?;
    check_entry_invariants(root, cwd, &entry)
}

/// finalize 的入口门禁：有绑定 id 就执行该条目；未绑定 id 时若仓内存在任何不变式声明则拒绝，
/// 防止旧调用方在新增冻结字段后静默跳过断言。
pub(crate) fn check_finalize_invariants(
    root: &Path,
    cwd: &Path,
    entry_id: Option<&str>,
) -> Result<(), String> {
    if let Some(entry_id) = entry_id.filter(|id| !id.trim().is_empty()) {
        return check_entry_invariants_by_id(root, cwd, entry_id);
    }
    let mut declared = Vec::new();
    for kind in [&REQUIREMENTS, &DEFECTS] {
        let store = DocStore::open(root, kind);
        let mut entries = store.load().map_err(|error| error.to_string())?;
        entries.extend(store.load_archive().map_err(|error| error.to_string())?);
        declared.extend(
            entries
                .iter()
                .filter(|entry| invariant_text(entry).is_some())
                .map(|entry| entry.id.clone()),
        );
    }
    if declared.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "finalize 发现已登记冻结不变式({}),但未提供 requirement_id；请绑定本次交付条目后重试。",
            declared.join(", ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("kz-r311-invariant-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn finalize_requires_entry_binding_when_invariants_exist() {
        let root = root("finalize-binding");
        let entry = Entry {
            id: "R-311".into(),
            title: "test".into(),
            status: "doing".into(),
            severity: None,
            fields: vec![(
                "不变式".into(),
                r#"[{"kind":"grep","path":"state.txt","pattern":"ready"}]"#.into(),
            )],
        };
        std::fs::write(root.join("state.txt"), "ready\n").unwrap();
        std::fs::create_dir_all(root.join(".kanzei/project")).unwrap();
        DocStore::open(&root, &REQUIREMENTS)
            .save(std::slice::from_ref(&entry))
            .unwrap();
        let unbound = check_finalize_invariants(&root, &root, None).unwrap_err();
        assert!(unbound.contains("requirement_id"), "{unbound}");
        assert!(check_finalize_invariants(&root, &root, Some("R-311")).is_ok());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn grep_invariant_passes_and_failure_names_assertion() {
        let root = root("grep");
        std::fs::write(root.join("state.txt"), "ready\n").unwrap();
        let entry = Entry {
            id: "R-311".into(),
            title: "test".into(),
            status: "doing".into(),
            severity: None,
            fields: vec![(
                "不变式".into(),
                r#"[{"kind":"grep","path":"state.txt","pattern":"^ready$"}]"#.into(),
            )],
        };
        assert!(check_entry_invariants(&root, &root, &entry).is_ok());
        let failed = Entry {
            fields: vec![(
                "不变式".into(),
                r#"[{"kind":"grep","path":"state.txt","pattern":"^done$"}]"#.into(),
            )],
            ..entry
        };
        let error = check_entry_invariants(&root, &root, &failed).unwrap_err();
        assert!(error.contains("#1"));
        assert!(error.contains("grep 未匹配"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn invalid_invariant_path_is_rejected_before_read() {
        let root = root("path");
        let entry = Entry {
            id: "D-1".into(),
            title: "test".into(),
            status: "fixing".into(),
            severity: Some("low".into()),
            fields: vec![(
                "不变式".into(),
                r#"[{"kind":"grep","path":"../outside","pattern":"x"}]"#.into(),
            )],
        };
        let error = check_entry_invariants(&root, &root, &entry).unwrap_err();
        assert!(error.contains("安全相对路径"));
        std::fs::remove_dir_all(root).ok();
    }
}
