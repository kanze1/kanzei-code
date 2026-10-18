//! Evidence addresses only the source spans granted to this model call.
use super::{DreamChunk, DreamFragmentKind};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DreamEvidence {
    /// Zero-based index in this call's spans array.
    pub span: usize,
    /// Exact source quotation; source identity and byte offsets are computed by the engine.
    pub quote: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidatedDreamEvidence {
    pub snapshot_sha256: String,
    pub source_id: String,
    pub sequence: i64,
    pub at_ms: i64,
    pub start_byte: usize,
    pub end_byte: usize,
    pub kind: DreamFragmentKind,
    pub input_origin: Option<String>,
    pub quote: String,
}

impl DreamChunk {
    pub fn validate_evidence(
        &self,
        evidence: &DreamEvidence,
    ) -> anyhow::Result<ValidatedDreamEvidence> {
        anyhow::ensure!(!evidence.quote.trim().is_empty(), "来源引用不能为空");
        let span = self
            .spans
            .get(evidence.span)
            .ok_or_else(|| anyhow::anyhow!("引用不属于当前片段"))?;
        let offset = span
            .text
            .find(&evidence.quote)
            .ok_or_else(|| anyhow::anyhow!("引用与冻结原文不一致"))?;
        let start_byte = span.start_byte + offset;
        let end_byte = start_byte + evidence.quote.len();
        anyhow::ensure!(end_byte <= span.end_byte, "引用越过当前片段");
        Ok(ValidatedDreamEvidence {
            snapshot_sha256: self.snapshot_sha256.clone(),
            source_id: span.source_id.clone(),
            sequence: span.sequence,
            at_ms: span.at_ms,
            start_byte,
            end_byte,
            kind: span.kind,
            input_origin: span.input_origin.clone(),
            quote: evidence.quote.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_dream::{DreamFragment, DreamSnapshot};
    #[test]
    fn evidence_cannot_invent_quotes_sources_or_human_origin() {
        let snapshot = DreamSnapshot {
            format_version: 1,
            session_id: "s".into(),
            segment_floor: 0,
            end_sequence: 2,
            fragments: vec![DreamFragment {
                source_id: "assistant-event".into(),
                sequence: 2,
                at_ms: 1,
                kind: DreamFragmentKind::Assistant,
                input_origin: None,
                text: "助手推断：可能喜欢紧凑排版。".into(),
            }],
        };
        let chunk = snapshot.chunks(2048).unwrap().remove(0);
        let valid = chunk
            .validate_evidence(&DreamEvidence {
                span: 0,
                quote: "可能喜欢紧凑排版".into(),
            })
            .unwrap();
        assert_eq!(valid.kind, DreamFragmentKind::Assistant);
        assert_eq!(valid.input_origin, None);
        assert_eq!(
            snapshot.fragments[0]
                .text
                .get(valid.start_byte..valid.end_byte),
            Some(valid.quote.as_str())
        );
        assert!(chunk
            .validate_evidence(&DreamEvidence {
                span: 1,
                quote: "可能喜欢紧凑排版".into()
            })
            .is_err());
        assert!(chunk
            .validate_evidence(&DreamEvidence {
                span: 0,
                quote: "用户明确要求所有项目都使用这种排版".into()
            })
            .is_err());
        assert!(serde_json::from_value::<DreamEvidence>(
            serde_json::json!({"span":0,"quote":"可能喜欢紧凑排版","input_origin":"user"})
        )
        .is_err());
    }
}
