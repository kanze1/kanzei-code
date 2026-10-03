//! Small requirement contract over the existing Markdown source of truth.
//! Runtime history stays in legacy fields and is never rewritten by spec edits.

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::Entry;

pub const SPEC_FIELDS: &[&str] = &[
    "需求格式",
    "需求类型",
    "内容",
    "验收",
    "来源",
    "开放问题",
    "需求关联",
];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequirementSpec {
    /// functional or non_functional; omit while the draft is unclassified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<RequirementKind>,
    #[serde(default)]
    pub statement: String,
    #[serde(default)]
    pub acceptance: Vec<AcceptanceCriterion>,
    #[serde(default)]
    pub source: RequirementSource,
    /// Unresolved questions affecting implementation or acceptance. Empty means resolved.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub questions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<RequirementLink>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RequirementKind {
    Functional,
    NonFunctional,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequirementSource {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reference: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub quote: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceCriterion {
    /// Keep existing IDs on update. For new criteria, omit to allocate an AC ID.
    #[serde(default)]
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequirementLink {
    pub relation: RequirementRelation,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RequirementRelation {
    Parent,
    DependsOn,
    Design,
    Related,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceEvidence {
    pub criterion_id: String,
    /// Copy the revision returned by req get; edits invalidate old evidence.
    pub revision: String,
    pub reference: String,
}

fn field<'a>(entry: &'a Entry, key: &str) -> &'a str {
    entry
        .fields
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
        .unwrap_or("")
}

impl RequirementSpec {
    pub fn from_entry(entry: &Entry) -> Result<Option<Self>, String> {
        if field(entry, "需求格式") != "2" {
            return Ok(None);
        }
        let parse = |key: &str| -> Result<serde_json::Value, String> {
            let raw = field(entry, key);
            serde_json::from_str(if raw.is_empty() { "null" } else { raw })
                .map_err(|e| format!("{} 的 {key} 格式无效: {e}", entry.id))
        };
        serde_json::from_value(serde_json::json!({
            "kind": if field(entry, "需求类型").is_empty() { None } else { Some(field(entry, "需求类型")) },
            "statement": field(entry, "内容"),
            "acceptance": if field(entry, "验收").is_empty() { serde_json::json!([]) } else { parse("验收")? },
            "source": if field(entry, "来源").is_empty() { serde_json::json!({}) } else { parse("来源")? },
            "questions": if field(entry, "开放问题").is_empty() { serde_json::json!([]) } else { parse("开放问题")? },
            "links": if field(entry, "需求关联").is_empty() { serde_json::json!([]) } else { parse("需求关联")? },
        })).map(Some).map_err(|e| format!("{} 的需求格式无效: {e}", entry.id))
    }

    pub fn normalize(&mut self, previous: Option<&Self>) -> Result<(), String> {
        let mut used: BTreeSet<String> = self
            .acceptance
            .iter()
            .filter(|c| !c.id.is_empty())
            .map(|c| c.id.clone())
            .collect();
        if used.len() != self.acceptance.iter().filter(|c| !c.id.is_empty()).count() {
            return Err("验收项 ID 重复；每项必须使用独立且稳定的 AC ID".into());
        }
        // Reserve retired IDs too, so a newly inserted criterion cannot inherit old evidence.
        if let Some(old) = previous {
            used.extend(old.acceptance.iter().map(|c| c.id.clone()));
        }
        let mut next = 1;
        for criterion in &mut self.acceptance {
            criterion.text = criterion.text.trim().into();
            if criterion.text.is_empty() {
                return Err("验收项内容不能为空；未知事项请放入 questions".into());
            }
            if criterion.id.is_empty() {
                if let Some(old) =
                    previous.and_then(|s| s.acceptance.iter().find(|c| c.text == criterion.text))
                {
                    return Err(format!("更新现有验收项时请保留 ID {}", old.id));
                }
                while used.contains(&format!("AC-{next}")) {
                    next += 1;
                }
                criterion.id = format!("AC-{next}");
                used.insert(criterion.id.clone());
            }
            if !criterion
                .id
                .strip_prefix("AC-")
                .is_some_and(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
            {
                return Err(format!("验收项 ID `{}` 应为 AC-数字", criterion.id));
            }
        }
        for link in &self.links {
            if link.target.trim().is_empty() {
                return Err("关联目标不能为空".into());
            }
        }
        if self.questions.iter().any(|q| q.trim().is_empty()) {
            return Err("开放问题不能为空字符串".into());
        }
        self.statement = self.statement.trim().into();
        Ok(())
    }

    pub fn gaps(&self) -> Vec<String> {
        let mut gaps = Vec::new();
        if self.statement.trim().is_empty() {
            gaps.push("补充需求正文".into());
        }
        if self.acceptance.is_empty() {
            gaps.push("补充可观测的验收标准".into());
        }
        if self.source.reference.trim().is_empty() && self.source.quote.trim().is_empty() {
            gaps.push("补充可回查的来源或原文".into());
        }
        gaps.extend(self.questions.iter().cloned());
        gaps
    }

    pub fn revision(&self) -> String {
        // Fingerprint the normative text, not scheduling, evidence, or display order.
        let mut acceptance = self.acceptance.clone();
        acceptance.sort_by(|a, b| a.id.cmp(&b.id));
        let text = serde_json::to_vec(&(&self.kind, &self.statement, acceptance)).unwrap();
        let hash = text.into_iter().fold(0xcbf29ce484222325u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
        });
        format!("spec-{hash:016x}")
    }

    pub fn fields(&self) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("需求格式".into(), "2".into()),
            (
                "需求类型".into(),
                self.kind
                    .as_ref()
                    .map(|kind| match kind {
                        RequirementKind::Functional => "functional",
                        RequirementKind::NonFunctional => "non_functional",
                    })
                    .unwrap_or("")
                    .into(),
            ),
            ("内容".into(), self.statement.clone()),
            (
                "验收".into(),
                serde_json::to_string(&self.acceptance).unwrap(),
            ),
            ("来源".into(), serde_json::to_string(&self.source).unwrap()),
            (
                "开放问题".into(),
                if self.questions.is_empty() {
                    String::new()
                } else {
                    serde_json::to_string(&self.questions).unwrap()
                },
            ),
            (
                "需求关联".into(),
                if self.links.is_empty() {
                    String::new()
                } else {
                    serde_json::to_string(&self.links).unwrap()
                },
            ),
        ])
    }
}

pub fn requirement_dependencies(entry: &Entry) -> Vec<String> {
    RequirementSpec::from_entry(entry)
        .ok()
        .flatten()
        .map(|spec| {
            spec.links
                .into_iter()
                .filter(|link| link.relation == RequirementRelation::DependsOn)
                .map(|link| link.target)
                .collect()
        })
        .unwrap_or_default()
}

pub fn acceptance_evidence(entry: &Entry) -> Result<Vec<AcceptanceEvidence>, String> {
    let raw = field(entry, "验收证据");
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(raw).map_err(|e| format!("{} 的验收证据格式无效: {e}", entry.id))
}

pub fn requirement_view(entry: &Entry) -> serde_json::Value {
    match RequirementSpec::from_entry(entry) {
        Ok(Some(spec)) => serde_json::json!({
            "format": 2, "revision": spec.revision(), "gaps": spec.gaps(),
            "spec": spec, "evidence": acceptance_evidence(entry).unwrap_or_default(),
            "evidence_error": acceptance_evidence(entry).err(),
        }),
        Ok(None) => serde_json::json!({"format": 1}),
        Err(error) => serde_json::json!({"format": 2, "error": error, "gaps": [error]}),
    }
}
