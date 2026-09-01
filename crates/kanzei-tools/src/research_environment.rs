//! Research 环境登记表解析与校验（R-345 B1）。
//!
//! `.kanzei/research/environments.md` 是人工维护的声明真源；本模块只解析、校验，
//! 不会把运行时快照写回登记表，也不会读取或保存真实凭据。

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResearchEnvironment {
    pub id: String,
    pub status: String,
    pub kind: String,
    pub host: String,
    pub owner: String,
    pub policy: String,
    pub gpu: String,
    pub workdir: String,
    pub runtime_limit: String,
    pub billing: String,
    pub credential_ref: String,
    pub preparation_steps: String,
    pub notes: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentDiagnostic {
    pub line: usize,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentConfigError {
    pub diagnostics: Vec<EnvironmentDiagnostic>,
}

impl fmt::Display for EnvironmentConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, diagnostic) in self.diagnostics.iter().enumerate() {
            if index > 0 {
                write!(f, "; ")?;
            }
            write!(f, "第 {} 行: {}", diagnostic.line, diagnostic.message)?;
        }
        Ok(())
    }
}

impl std::error::Error for EnvironmentConfigError {}

pub fn environments_path(root: &Path) -> PathBuf {
    root.join(".kanzei/research/environments.md")
}

pub fn load_environment(root: &Path, environment_id: &str) -> Result<ResearchEnvironment, String> {
    let path = environments_path(root);
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("读取环境登记表 {} 失败: {error}", path.display()))?;
    let environments = parse_environments_markdown(&text).map_err(|error| error.to_string())?;
    environments
        .into_iter()
        .find(|environment| environment.id == environment_id)
        .ok_or_else(|| format!("环境登记表中未找到 `{environment_id}`"))
}

pub fn parse_environments_markdown(
    text: &str,
) -> Result<Vec<ResearchEnvironment>, EnvironmentConfigError> {
    type RawFields = BTreeMap<String, (String, usize)>;
    type RawEntry = (String, String, usize, RawFields);
    let mut current: Option<RawEntry> = None;
    let mut result = Vec::new();
    let mut diagnostics = Vec::new();

    for (index, raw_line) in text.lines().enumerate() {
        let line_number = index + 1;
        let line = raw_line.trim();
        if let Some(heading) = line.strip_prefix("## ") {
            if let Some(entry) = current.take() {
                validate_entry(entry, &mut result, &mut diagnostics);
            }
            let (id, status) = parse_heading(heading, line_number, &mut diagnostics);
            current = Some((id, status, line_number, BTreeMap::new()));
        } else if let Some(field) = line.strip_prefix("- ") {
            let Some((key, value)) = field.split_once(':') else {
                diagnostics.push(EnvironmentDiagnostic {
                    line: line_number,
                    message: "环境字段必须是 `- key: value`".into(),
                });
                continue;
            };
            if let Some((_, _, _, fields)) = current.as_mut() {
                fields.insert(
                    key.trim().to_owned(),
                    (value.trim().to_owned(), line_number),
                );
            }
        }
    }
    if let Some(entry) = current.take() {
        validate_entry(entry, &mut result, &mut diagnostics);
    }
    if result.is_empty() && diagnostics.is_empty() {
        diagnostics.push(EnvironmentDiagnostic {
            line: 1,
            message: "环境登记表没有任何 `## ENV-*` 环境段".into(),
        });
    }
    if diagnostics.is_empty() {
        Ok(result)
    } else {
        Err(EnvironmentConfigError { diagnostics })
    }
}

fn parse_heading(
    heading: &str,
    line: usize,
    diagnostics: &mut Vec<EnvironmentDiagnostic>,
) -> (String, String) {
    let (id, status) = heading
        .split_once(" [")
        .map(|(id, rest)| (id.trim(), rest.strip_suffix(']').unwrap_or(rest).trim()))
        .unwrap_or((heading.trim(), "active"));
    if !id.starts_with("ENV-") || id.len() <= 4 {
        diagnostics.push(EnvironmentDiagnostic {
            line,
            message: format!("环境 id `{id}` 必须是 ENV-<name>"),
        });
    }
    if !matches!(status, "active" | "inactive" | "retired") {
        diagnostics.push(EnvironmentDiagnostic {
            line,
            message: format!("status `{status}` 无效，可用 active/inactive/retired"),
        });
    }
    (id.to_owned(), status.to_owned())
}

fn validate_entry(
    (id, status, heading_line, fields): (String, String, usize, BTreeMap<String, (String, usize)>),
    result: &mut Vec<ResearchEnvironment>,
    diagnostics: &mut Vec<EnvironmentDiagnostic>,
) {
    let get = |keys: &[&str]| -> (String, usize) {
        keys.iter()
            .find_map(|key| fields.get(*key).cloned())
            .unwrap_or_default()
    };
    let (kind, kind_line) = get(&["kind"]);
    let (host, _host_line) = get(&["host"]);
    let (owner, _) = get(&["归属", "owner"]);
    let (policy, policy_line) = get(&["执行策略", "policy"]);
    let (gpu, _) = get(&["gpu"]);
    let (workdir, _workdir_line) = get(&["workdir"]);
    let (runtime_limit, _) = get(&["运行时限", "runtime_limit"]);
    let (billing, _) = get(&["计费", "billing"]);
    let (credential_ref, credential_line) = get(&["凭据引用", "credential_ref"]);
    let (preparation_steps, preparation_line) = get(&["准备步骤", "preparation_steps"]);
    let (notes, _) = get(&["备注", "notes"]);

    if kind.is_empty() {
        diagnostics.push(EnvironmentDiagnostic {
            line: heading_line,
            message: "缺少 kind".into(),
        });
    } else if !matches!(kind.as_str(), "local" | "ssh") {
        diagnostics.push(EnvironmentDiagnostic {
            line: kind_line,
            message: format!("kind `{kind}` 无效，可用 local/ssh"),
        });
    }
    if kind == "ssh" && host.is_empty() {
        diagnostics.push(EnvironmentDiagnostic {
            line: heading_line,
            message: "ssh 环境缺少 host".into(),
        });
    }
    if policy.is_empty() {
        diagnostics.push(EnvironmentDiagnostic {
            line: heading_line,
            message: "缺少执行策略/policy".into(),
        });
    } else if !matches!(
        policy.as_str(),
        "relaxed" | "managed" | "approval" | "strict"
    ) {
        diagnostics.push(EnvironmentDiagnostic {
            line: policy_line,
            message: format!("执行策略 `{policy}` 无效，可用 relaxed/managed/approval/strict"),
        });
    }
    if workdir.is_empty() {
        diagnostics.push(EnvironmentDiagnostic {
            line: heading_line,
            message: "缺少 workdir".into(),
        });
    }
    if preparation_steps.is_empty() {
        diagnostics.push(EnvironmentDiagnostic {
            line: preparation_line.max(heading_line),
            message: "缺少准备步骤".into(),
        });
    }
    if !credential_ref.is_empty() && !credential_ref.starts_with("secret://") {
        diagnostics.push(EnvironmentDiagnostic {
            line: credential_line,
            message: "凭据引用必须是 secret:// URI，不得写入真实凭据".into(),
        });
    }
    if credential_ref.contains("BEGIN ") || credential_ref.contains("password=") {
        diagnostics.push(EnvironmentDiagnostic {
            line: credential_line,
            message: "检测到疑似真实凭据，登记表只允许 secret:// 引用".into(),
        });
    }
    result.push(ResearchEnvironment {
        id,
        status,
        kind,
        host,
        owner,
        policy,
        gpu,
        workdir,
        runtime_limit,
        billing,
        credential_ref,
        preparation_steps,
        notes,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = "# Environments\n\n## ENV-gpu01 [active]\n- kind: ssh\n- host: user@10.0.0.11\n- 归属: personal\n- 执行策略: relaxed\n- gpu: 4 × RTX 4090 24G\n- workdir: /data/exp\n- 运行时限: 24h\n- 计费: 自有\n- 凭据引用: secret://gpu01/ssh\n- 准备步骤: 首次人工完成 git clone，后续复用\n- 备注: test\n";

    #[test]
    fn parses_declared_environment_without_exposing_secret() {
        let environments = parse_environments_markdown(VALID).unwrap();
        assert_eq!(environments[0].id, "ENV-gpu01");
        assert_eq!(environments[0].policy, "relaxed");
        assert_eq!(environments[0].credential_ref, "secret://gpu01/ssh");
        assert!(!environments[0].credential_ref.contains("private"));
    }

    #[test]
    fn reports_invalid_policy_and_missing_preparation_with_lines() {
        let text = "## ENV-bad [active]\n- kind: ssh\n- host: user@host\n- 执行策略: unsafe\n- workdir: /tmp\n- 凭据引用: password=secret\n";
        let error = parse_environments_markdown(text).unwrap_err();
        assert!(error
            .diagnostics
            .iter()
            .any(|item| item.line == 4 && item.message.contains("执行策略")));
        assert!(error
            .diagnostics
            .iter()
            .any(|item| item.line == 1 && item.message.contains("准备步骤")));
        assert!(error
            .diagnostics
            .iter()
            .any(|item| item.line == 6 && item.message.contains("secret://")));
    }
}
