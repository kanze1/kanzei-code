//! 成功切换工作条目后的无损 observation 收纳。原始事实、用户指令和工具配对不变。
use std::collections::{HashMap, VecDeque};
use std::sync::{atomic::Ordering, Arc};
use std::time::{Duration, Instant};

use kanzei_harness::ToolCtx;
use kanzei_llm::{Message, Part};
use serde_json::{json, Value};

use super::{
    context::estimate_prompt_tokens, tool_exec::archive_context_result, RunEvent, WorkContextReport,
};

const MARKER: &str = "_kanzei_context_boundary";
const MIN_SAVING_TOKENS: u64 = 4_096;
const RECENT_TOKENS: u64 = 4_096;
const RELATED_RECENT_TOKENS: u64 = 12_288;
const MAX_RESULTS: usize = 16;

#[derive(Default)]
pub(super) struct ItemContext {
    // 保存失败/收益不足也只在本轮尝试一次，不把整理变成重试循环。
    attempted: Option<(usize, usize, String)>,
}

#[derive(Clone)]
struct Claim {
    id: String,
    call_message: usize,
    result: (usize, usize),
    receipt: Value,
}

struct Observation {
    result: (usize, usize),
}

struct Boundary {
    from: Claim,
    to: Claim,
    observations: Vec<Observation>,
}

struct Call<'a> {
    message: usize,
    name: &'a str,
    input: &'a Value,
}

fn claim(call: &Call<'_>, result: (usize, usize), content: &str) -> Option<Claim> {
    if call.name != "work" || call.input["action"] != "claim" {
        return None;
    }
    let id = call.input["id"].as_str()?;
    if id.is_empty() || id.len() > 80 {
        return None;
    }
    let receipt: Value = serde_json::from_str(content).ok()?;
    let legacy = receipt["claimed"] == id
        && matches!(
            receipt["lifecycle_status"].as_str(),
            Some("doing" | "fixing")
        );
    let unit = receipt["unit_id"] == id
        && matches!(receipt["status"].as_str(), Some("active" | "verifying"));
    (legacy || unit).then(|| Claim {
        id: id.into(),
        call_message: call.message,
        result,
        receipt,
    })
}

fn protected_path(input: &Value) -> bool {
    let path = input["path"]
        .as_str()
        .unwrap_or("")
        .replace('\\', "/")
        .to_lowercase();
    path.split('/')
        .any(|part| matches!(part, ".kanzei" | ".codex" | "agents.md" | "skill.md"))
}

fn can_archive(call: &Call<'_>, content: &str) -> bool {
    matches!(call.name, "read" | "grep" | "glob")
        && !protected_path(call.input)
        && content.len() >= 2_048
        && !content.starts_with("[tool_outcome=")
        && !content.starts_with("[tool_result_")
        && !content.starts_with("[work_context_archived")
        && !content.contains("[tool-image]")
}

/// 只信任已配对的 work claim 成功收据；查询、文本声明、失败和其他工具不能触发。
fn latest_boundary(messages: &[Message]) -> Option<Boundary> {
    let mut pending: HashMap<&str, VecDeque<Call<'_>>> = HashMap::new();
    let mut active: Option<Claim> = None;
    let mut change = None;
    let mut observations = Vec::new();
    for (mi, message) in messages.iter().enumerate() {
        for (pi, part) in message.parts.iter().enumerate() {
            match part {
                Part::ToolCall { id, name, input } => {
                    pending.entry(id).or_default().push_back(Call {
                        message: mi,
                        name,
                        input,
                    });
                }
                Part::ToolResult {
                    call_id,
                    content,
                    is_error,
                } => {
                    let Some(call) = pending
                        .get_mut(call_id.as_str())
                        .and_then(VecDeque::pop_front)
                    else {
                        continue;
                    };
                    if *is_error {
                        continue;
                    }
                    if let Some(next) = claim(&call, (mi, pi), content) {
                        if active.as_ref().is_some_and(|old| old.id == next.id) {
                            continue;
                        }
                        if let Some(previous) = active.replace(next.clone()) {
                            change = Some((previous, next));
                        }
                    } else if can_archive(&call, content) {
                        observations.push(Observation { result: (mi, pi) });
                    }
                }
                _ => {}
            }
        }
    }
    // 工具组尚未完整返回时不产生新投影。
    if pending.values().any(|calls| !calls.is_empty()) {
        return None;
    }
    let (from, to) = change?;
    if to.receipt[MARKER]["version"] == 1
        && to.receipt[MARKER]["from"] == from.id
        && to.receipt[MARKER]["to"] == to.id
    {
        return None;
    }
    observations.retain(|observation| observation.result.0 < to.call_message);
    Some(Boundary {
        from,
        to,
        observations,
    })
}

fn items(value: &Value) -> impl Iterator<Item = &str> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
}

fn related(a: &Claim, b: &Claim) -> bool {
    let parent = |claim: &Claim| claim.id.split('/').next().unwrap_or("").to_string();
    if parent(a) == parent(b) {
        return true;
    }
    for (source, target) in [(a, b), (b, a)] {
        if items(&source.receipt["dependencies"])
            .chain(items(&source.receipt["context"]["related_items"]))
            .any(|id| id == target.id || id == parent(target))
        {
            return true;
        }
    }
    items(&a.receipt["scope"]).any(|a_scope| {
        let a_scope = a_scope.replace('\\', "/");
        items(&b.receipt["scope"]).any(|b_scope| {
            let b_scope = b_scope.replace('\\', "/");
            a_scope == b_scope
                || a_scope.starts_with(&format!("{b_scope}/"))
                || b_scope.starts_with(&format!("{a_scope}/"))
        })
    })
}

fn result_content(messages: &[Message], position: (usize, usize)) -> &str {
    match &messages[position.0].parts[position.1] {
        Part::ToolResult { content, .. } => content,
        _ => unreachable!("positions come from paired tool results"),
    }
}

impl ItemContext {
    pub(super) fn prepare(
        &mut self,
        messages: &mut Vec<Message>,
        ctx: &ToolCtx,
        on_event: &mut (dyn FnMut(RunEvent) + Send),
    ) {
        let Some(boundary) = latest_boundary(messages) else {
            return;
        };
        let key = (
            boundary.to.result.0,
            boundary.to.result.1,
            boundary.to.id.clone(),
        );
        if self.attempted.as_ref() == Some(&key) {
            return;
        }
        self.attempted = Some(key);
        let related = related(&boundary.from, &boundary.to);
        let protect = if related {
            RELATED_RECENT_TOKENS
        } else {
            RECENT_TOKENS
        };
        let mut cutoff = boundary.to.call_message;
        let mut recent = 0;
        // 以认领位置为锚，不能随继续调试而向后移动清理窗口。
        while cutoff > 0 && recent < protect {
            cutoff -= 1;
            recent += estimate_prompt_tokens(&[], &messages[cutoff..cutoff + 1], &[]);
        }
        // 同时保留至少两份最近的大观察，避免一个巨大结果独占 token 窗口。
        let older = boundary.observations.len().saturating_sub(2);
        let candidates: Vec<_> = boundary.observations[..older]
            .iter()
            .filter(|observation| observation.result.0 < cutoff)
            .take(MAX_RESULTS)
            .collect();
        let gain: u64 = candidates
            .iter()
            .map(|o| result_content(messages, o.result).len().saturating_sub(512) as u64 / 4)
            .sum();
        if gain < MIN_SAVING_TOKENS {
            return;
        }

        let mut surface = messages.clone();
        let start = Instant::now();
        let mut archived_results = 0;
        for observation in candidates {
            if start.elapsed() >= Duration::from_millis(250) {
                break;
            }
            let (mi, pi) = observation.result;
            let Part::ToolResult { content, .. } = &mut surface[mi].parts[pi] else {
                unreachable!()
            };
            if let Some(reference) = archive_context_result(&ctx.project_root, content) {
                *content = reference;
                archived_results += 1;
            } else {
                break;
            }
        }
        if archived_results == 0 {
            return;
        }
        let mut receipt = boundary.to.receipt;
        receipt[MARKER] = json!({
            "version": 1, "from": boundary.from.id, "to": boundary.to.id,
            "archived_results": archived_results,
        });
        if let Part::ToolResult { content, .. } =
            &mut surface[boundary.to.result.0].parts[boundary.to.result.1]
        {
            *content = receipt.to_string();
        }
        let before_tokens = estimate_prompt_tokens(&[], messages, &[]);
        let after_tokens = estimate_prompt_tokens(&[], &surface, &[]);
        if before_tokens.saturating_sub(after_tokens) < MIN_SAVING_TOKENS {
            return;
        }
        let accepted = Arc::new(std::sync::atomic::AtomicBool::new(false));
        on_event(RunEvent::WorkContextPrepared {
            report: WorkContextReport {
                from_item: boundary.from.id,
                to_item: boundary.to.id,
                related,
                archived_results,
                before_tokens,
                after_tokens,
            },
            surface: surface.clone(),
            accepted: Arc::clone(&accepted),
        });
        if accepted.load(Ordering::Acquire) {
            *messages = surface;
        }
    }
}

#[cfg(test)]
mod tests;
