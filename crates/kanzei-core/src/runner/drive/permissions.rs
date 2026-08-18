//! 串行工具执行的权限门禁。
//!
//! 该模块只负责 ruleset、session 与用户 ASK 的决策，并回传 `Gate`；调用方继续
//! 负责拒绝结果、停止收尾、工具执行、ToolEnd 事件和结果索引配对。

use super::*;

pub(super) struct PermissionGateRequest<'a> {
    pub(super) config: &'a RunnerConfig,
    pub(super) snapshot: &'a HarnessSnapshot,
    pub(super) tool: &'a dyn Tool,
    pub(super) input: &'a serde_json::Value,
    pub(super) id: &'a str,
    pub(super) ctx: &'a ToolCtx,
    pub(super) on_event: &'a mut (dyn FnMut(RunEvent) + Send),
    pub(super) ask: &'a mut (dyn FnMut(AskRequest) -> AskFuture + Send),
    pub(super) session_approved: &'a mut std::collections::HashSet<(String, String)>,
    pub(super) session_rules: &'a mut Vec<(String, String)>,
}

pub(super) async fn resolve_permission_gate(state: &mut PermissionGateRequest<'_>) -> Gate {
    let action = state.tool.action();
    let mut gate_result = Gate::Pass;
    let mut pending_ask: Vec<String> = Vec::new();
    for resource in state.tool.resources_with_ctx(state.input, state.ctx) {
        // 路径类资源:统一正斜杠 + 消解 . / ..,权限 pattern 不用关心平台,也不能
        // 被路径变体绕过:`.kanzei/research/../../src/main.rs` 会被
        // `*.kanzei/research/*` 判为放行,而落盘时 join 会消解 ..,实际写到项目
        // 任意位置(D-050)。
        // bash 资源是 shell 文本,同一套规范化在它身上是提权通道(D-269):
        // `..` 会把前一段整段弹掉,注入语句藏在被弹掉的那一段里。这里落到
        // session_rules 的 pattern 也是本函数的产物——bash 走原样,注入段里的
        // `*` 才能活到 pattern 成形,D-051 的串联降级才不会被绕开。
        let normalized =
            kanzei_harness::permission::normalize_resource_for_action(action, &resource);
        // R-183:ruleset 判定带命中的规则原文(验收④轨迹)。
        let mut resolved = |decision, source, rule: Option<String>| {
            (state.on_event)(RunEvent::PermissionResolved {
                tool_call_id: state.id.to_owned(),
                action: action.to_string(),
                resource: normalized.clone(),
                decision,
                source,
                rule,
            });
        };
        match state.snapshot.evaluate_with_rule(action, &normalized) {
            (Effect::Deny, rule) => {
                resolved("deny", "ruleset", rule.map(super::describe_rule));
                gate_result = Gate::Deny(normalized);
                break;
            }
            (Effect::Ask, _) => pending_ask.push(normalized),
            (Effect::Allow, rule) => {
                resolved("allow", "ruleset", rule.map(super::describe_rule));
            }
        }
    }
    if matches!(gate_result, Gate::Pass) {
        for resource in pending_ask {
            let key = (action.to_string(), resource.clone());
            let mut resolved = |decision, source| {
                (state.on_event)(RunEvent::PermissionResolved {
                    tool_call_id: state.id.to_owned(),
                    action: action.to_string(),
                    resource: resource.clone(),
                    decision,
                    source,
                    // R-183:会话层/策略层决策无规则原文可归属。
                    rule: None,
                });
            };
            if state.session_approved.contains(&key) {
                resolved("allow", "session_approved");
                continue;
            }
            if state.session_rules.iter().any(|(a, pattern)| {
                a == action
                    && kanzei_harness::permission::resource_match_for_action(a, pattern, &resource)
            }) {
                resolved("allow", "session_rule");
                continue;
            }
            match state.config.ask_policy {
                // D-281:自动放行——权限询问直接放行并落事件,不短路、
                // 不再需要前端替答(前端 07-events.js 只处理 Interactive 轮)。
                AskPolicy::AutoAllow => {
                    resolved("allow", "auto_allow");
                    continue;
                }
                _ if !state.config.ask_policy.allows_user_prompt() => {
                    resolved("declined", "noninteractive");
                    gate_result = Gate::NonInteractive(format!(
                        "permission requires user approval: {action} on `{resource}`; autonomous/parallel run skipped it",
                    ));
                    break;
                }
                _ => {}
            }
            match (state.ask)(AskRequest::Permission {
                action: action.to_string(),
                resource: resource.clone(),
            })
            .await
            {
                AskResponse::Permission(AskReply::Deny)
                | AskResponse::Cancelled
                | AskResponse::Answer(_) => {
                    resolved("declined", "user");
                    gate_result = Gate::UserDeclined;
                    break;
                }
                AskResponse::Permission(AskReply::AllowOnce) => {
                    resolved("allow_once", "user");
                    state.session_approved.insert(key);
                }
                AskResponse::Permission(AskReply::AlwaysAllow) => {
                    resolved("always_allow", "user");
                    state.session_rules.push((
                        action.to_string(),
                        kanzei_harness::config::generalize_resource(action, &resource),
                    ));
                }
            }
        }
    }
    gate_result
}
