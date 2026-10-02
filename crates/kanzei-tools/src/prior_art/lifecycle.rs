//! 需求已登记后的先行调研状态与实施门禁；状态实时从证据读取，不缓存到字段。

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use crate::docstore::Entry;

use super::{expected_prior_art_path, metadata, validate_artifact};

pub(crate) const ARTIFACT_FIELD: &str = "先行调研";
pub(crate) const WAIVER_FIELD: &str = "先行调研豁免";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PriorArtStatus {
    Pending,
    Complete,
    Waived,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EntryPriorArtStatus {
    pub status: PriorArtStatus,
    pub path: Option<String>,
    pub issue: Option<String>,
}

fn audit_values<'a>(entry: &'a Entry, field: &str) -> Vec<&'a str> {
    entry
        .fields
        .iter()
        .filter(|(key, _)| key == field)
        .map(|(_, value)| value.trim())
        .collect()
}

fn waiver_valid(reason: &str) -> Result<(), String> {
    if reason.chars().count() < 8 {
        Err("prior_art_waiver 必须记录至少 8 个字符的明确用户理由，不能写空泛占位".into())
    } else {
        Ok(())
    }
}

/// 普通条目和未采用此审计字段的历史需求返回 None；不对存量需求追溯新增门禁。
/// complete 必须经过同一个工件验证器，不能仅信任 Markdown 的 status。
pub fn entry_status(root: &Path, entry: &Entry) -> Option<EntryPriorArtStatus> {
    if !entry.id.starts_with("R-") {
        return None;
    }
    let artifacts = audit_values(entry, ARTIFACT_FIELD);
    let waivers = audit_values(entry, WAIVER_FIELD);
    if artifacts.is_empty() && waivers.is_empty() {
        return None;
    }
    let path = artifacts.first().map(|path| (*path).to_string());
    let invalid = |issue: String| EntryPriorArtStatus {
        status: PriorArtStatus::Invalid,
        path: path.clone(),
        issue: Some(issue),
    };
    if artifacts.len() + waivers.len() != 1 {
        return Some(invalid(
            "先行调研审计字段重复或工件与豁免同时存在，请用顶层 prior_art/prior_art_waiver 修正"
                .into(),
        ));
    }
    if let Some(reason) = waivers.first() {
        return Some(match waiver_valid(reason) {
            Ok(()) => EntryPriorArtStatus {
                status: PriorArtStatus::Waived,
                path: None,
                issue: None,
            },
            Err(error) => invalid(error),
        });
    }
    let relative = artifacts[0];
    Some(match validate_artifact(root, relative, Some(&entry.id)) {
        Ok(_) => EntryPriorArtStatus {
            status: PriorArtStatus::Complete,
            path,
            issue: None,
        },
        Err(error) => {
            // 仅完整、归属正确的 pending 头视为待调研；坏路径/坏头仍显示 invalid。
            let pending = expected_prior_art_path(root, relative)
                .ok()
                .and_then(|(topic, absolute)| {
                    let text = std::fs::read_to_string(absolute).ok()?;
                    let meta = metadata(&text);
                    Some(
                        meta.get("kind").map(String::as_str) == Some("prior_art")
                            && meta.get("topic").map(String::as_str) == Some(topic.as_str())
                            && meta.get("status").map(String::as_str) == Some("pending")
                            && meta.get("entry_refs").is_some_and(|refs| {
                                refs.split_whitespace().any(|id| id == entry.id)
                            }),
                    )
                })
                .unwrap_or(false);
            EntryPriorArtStatus {
                status: if pending {
                    PriorArtStatus::Pending
                } else {
                    PriorArtStatus::Invalid
                },
                path,
                issue: Some(error),
            }
        }
    })
}

pub(crate) fn check_start(root: &Path, entry: &Entry) -> Result<(), String> {
    let Some(state) = entry_status(root, entry) else {
        return Ok(());
    };
    match state.status {
        PriorArtStatus::Complete | PriorArtStatus::Waived => Ok(()),
        PriorArtStatus::Pending | PriorArtStatus::Invalid => Err(format!(
            "{} 的先行调研尚未通过，需求已登记，可先完成调研。{} 工件: {}。补齐双侧对照并验证后，用 req update id={} status=doing 开始；用户明确跳过时，用顶层 prior_art_waiver 记录理由。",
            entry.id,
            state.issue.as_deref().unwrap_or(""),
            state.path.as_deref().unwrap_or("未提供"),
            entry.id,
        )),
    }
}

/// 审计链接不能通过自由字段清空、替换或伪造；只接受经过证据校验的顶层参数。
pub(crate) fn check_input_fields(fields: &BTreeMap<String, String>) -> Result<(), String> {
    if fields.keys().any(|key| {
        let key = key.trim();
        key == ARTIFACT_FIELD
            || key == WAIVER_FIELD
            || key.eq_ignore_ascii_case("prior_art")
            || key.eq_ignore_ascii_case("prior_art_waiver")
    }) {
        return Err("先行调研审计字段由工具维护，请使用顶层 prior_art 或 prior_art_waiver；不能通过 fields 删除或伪造".into());
    }
    Ok(())
}

pub(crate) fn update_audit(
    root: &Path,
    before: &Entry,
    artifact: Option<&str>,
    waiver: Option<&str>,
) -> Result<Option<(String, String)>, String> {
    if artifact.is_some() && waiver.is_some() {
        return Err(
            "prior_art 与 prior_art_waiver 互斥：要么提交工件，要么记录用户豁免理由".into(),
        );
    }
    if let Some(relative) = artifact.map(str::trim) {
        validate_artifact(root, relative, Some(&before.id))?;
        return Ok(Some((ARTIFACT_FIELD.into(), relative.into())));
    }
    if let Some(reason) = waiver.map(str::trim) {
        let existing = before
            .fields
            .iter()
            .any(|(key, _)| key == ARTIFACT_FIELD || key == WAIVER_FIELD);
        let core = before.fields.iter().any(|(key, value)| {
            (key == "标签" || key.eq_ignore_ascii_case("tags") || key.eq_ignore_ascii_case("tag"))
                && value
                    .split(|c: char| c == ',' || c.is_whitespace())
                    .any(|tag| tag == "核心")
        });
        if !(existing || core && before.refs().is_empty()) {
            return Err("prior_art_waiver 只用于已关联先行调研或「核心 + refs 为空」的需求".into());
        }
        waiver_valid(reason)?;
        return Ok(Some((WAIVER_FIELD.into(), reason.into())));
    }
    Ok(None)
}
