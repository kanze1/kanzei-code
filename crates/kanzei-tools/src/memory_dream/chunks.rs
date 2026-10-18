//! Partition exact redacted source spans under the serialized input budget.
use super::{DreamFragment, DreamFragmentKind, DreamSnapshot};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DreamSpan {
    pub source_id: String,
    pub sequence: i64,
    pub at_ms: i64,
    pub kind: DreamFragmentKind,
    pub input_origin: Option<String>,
    pub start_byte: usize,
    pub end_byte: usize,
    pub text: String,
}

impl DreamSpan {
    fn new(source: &DreamFragment, start: usize, end: usize) -> Self {
        Self {
            source_id: source.source_id.clone(),
            sequence: source.sequence,
            at_ms: source.at_ms,
            kind: source.kind,
            input_origin: source.input_origin.clone(),
            start_byte: start,
            end_byte: end,
            text: source.text[start..end].to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DreamChunk {
    pub snapshot_sha256: String,
    pub index: usize,
    pub spans: Vec<DreamSpan>,
}

impl DreamSnapshot {
    pub fn chunks(&self, max_bytes: usize) -> anyhow::Result<Vec<DreamChunk>> {
        let digest = self.digest()?;
        let mut chunks = Vec::new();
        let mut current = DreamChunk {
            snapshot_sha256: digest.clone(),
            index: 0,
            spans: Vec::new(),
        };
        for source in &self.fragments {
            let boundaries = source
                .text
                .char_indices()
                .map(|(index, _)| index)
                .chain(std::iter::once(source.text.len()))
                .collect::<Vec<_>>();
            let mut start_index = 0;
            while start_index + 1 < boundaries.len() {
                let mut lower = start_index;
                let mut upper = boundaries.len() - 1;
                while lower < upper {
                    let end_index = lower + (upper - lower).div_ceil(2);
                    current.spans.push(DreamSpan::new(
                        source,
                        boundaries[start_index],
                        boundaries[end_index],
                    ));
                    let fits = serde_json::to_vec(&current)?.len() <= max_bytes;
                    current.spans.pop();
                    if fits {
                        lower = end_index;
                    } else {
                        upper = end_index - 1;
                    }
                }
                if lower == start_index {
                    anyhow::ensure!(
                        !current.spans.is_empty(),
                        "做梦片段预算小于来源元数据和一个字符，不能无损切分"
                    );
                } else {
                    current.spans.push(DreamSpan::new(
                        source,
                        boundaries[start_index],
                        boundaries[lower],
                    ));
                    start_index = lower;
                }
                if start_index + 1 < boundaries.len() {
                    chunks.push(current);
                    current = DreamChunk {
                        snapshot_sha256: digest.clone(),
                        index: chunks.len(),
                        spans: Vec::new(),
                    };
                }
            }
        }
        if !current.spans.is_empty() {
            chunks.push(current);
        }
        Ok(chunks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn multilingual_long_source_is_complete_deterministic_and_within_serialized_budget() {
        let text = "偏好简洁的排版、具体的证据。\n\\\"path\" and emoji 🧪\n".repeat(120);
        let snapshot = DreamSnapshot {
            format_version: 1,
            session_id: "selected".into(),
            segment_floor: 0,
            end_sequence: 5,
            fragments: vec![DreamFragment {
                source_id: "event-5".into(),
                sequence: 5,
                at_ms: 1,
                kind: DreamFragmentKind::OriginalInput,
                input_origin: None,
                text: text.clone(),
            }],
        };
        let chunks = snapshot.chunks(1024).unwrap();
        assert!(chunks.len() > 2);
        let mut reconstructed = String::new();
        for (index, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.index, index);
            assert!(serde_json::to_vec(chunk).unwrap().len() <= 1024);
            for span in &chunk.spans {
                assert_eq!(span.start_byte, reconstructed.len());
                reconstructed.push_str(&span.text);
                assert_eq!(span.end_byte, reconstructed.len());
            }
        }
        assert_eq!(reconstructed, text);
        assert_eq!(snapshot.chunks(1024).unwrap(), chunks);
        assert!(
            snapshot.chunks(1).is_err(),
            "impossible budgets must not silently drop source text"
        );
    }
}
