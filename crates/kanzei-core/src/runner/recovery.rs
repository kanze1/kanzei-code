//! Shared, conservative recovery evidence for summaries and memory promotion.
use kanzei_llm::{Message, Part};
use std::collections::{BTreeMap, HashMap};

/// Bump whenever evidence requirements change; old observations remain auditable.
pub(crate) const RECOVERY_CONTRACT_VERSION: i64 = 1;

pub(crate) struct VerifiedRecovery {
    pub(crate) fingerprint: String,
    pub(crate) failed_call_id: String,
    pub(crate) recovered_call_id: String,
    pub(crate) tool: String,
    pub(crate) target: String,
}

pub(crate) fn verified_recoveries(messages: &[Message]) -> Vec<VerifiedRecovery> {
    let mut calls = HashMap::new();
    let mut pending = HashMap::<(String, String), (String, String)>::new();
    let mut recovered = BTreeMap::<(String, String), VerifiedRecovery>::new();
    for part in messages.iter().flat_map(|message| &message.parts) {
        match part {
            Part::ToolCall { id, name, input } => {
                // 完整目标及相同工具，避免 edit a/x.rs 后 read b/x.rs 被当成修复。
                let target = if name == "bash" {
                    input.get("command").and_then(serde_json::Value::as_str)
                } else if matches!(name.as_str(), "edit" | "write" | "insert" | "read") {
                    input.get("path").and_then(serde_json::Value::as_str)
                } else {
                    None
                };
                if let Some(target) = target.filter(|target| !target.trim().is_empty()) {
                    // Recovery is evidence for the same operation, not merely a file
                    // mentioned by a later successful tool. Keep the complete input:
                    // different ranges, replacements and workdirs are different goals.
                    calls.insert(
                        id.clone(),
                        ((name.clone(), input.to_string()), target.trim().to_string()),
                    );
                }
            }
            Part::ToolResult {
                call_id,
                content,
                is_error,
                ..
            } => {
                let Some((key, target)) = calls.get(call_id) else {
                    continue;
                };
                if *is_error {
                    recovered.remove(key);
                    pending.remove(key);
                    if crate::runner::metrics::is_expected_tool_rejection(content) {
                        continue;
                    }
                    let kind = crate::runner::metrics::failure_kind(content);
                    if !crate::is_usable_failure_kind(&kind) {
                        continue;
                    }
                    let fingerprint = crate::normalize_fp_marker(&format!("[fp:{}|{kind}]", key.0));
                    pending.insert(key.clone(), (call_id.clone(), fingerprint));
                } else if crate::runner::metrics::is_expected_tool_rejection(content) {
                    // noop 等受控结果不是成功恢复，也不能沿用之前未解决的失败。
                    pending.remove(key);
                    recovered.remove(key);
                } else if let Some((failed_call_id, fingerprint)) = pending.remove(key) {
                    recovered.insert(
                        key.clone(),
                        VerifiedRecovery {
                            fingerprint,
                            failed_call_id,
                            recovered_call_id: call_id.clone(),
                            tool: key.0.clone(),
                            target: target.clone(),
                        },
                    );
                }
            }
            _ => {}
        }
    }
    recovered.into_values().collect()
}
