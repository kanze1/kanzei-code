//! 工具执行共用的权限决策计划与串行询问门禁。
//!
//! 该模块只负责 ruleset、session 与用户 ASK 的决策，并回传 `Gate`；调用方继续
//! 负责拒绝结果、停止收尾、工具执行、ToolEnd 事件和结果索引配对。

use super::*;

struct ResolvedPermission {
    resource: String,
    decision: &'static str,
    source: &'static str,
    rule: Option<String>,
}

pub(super) struct PendingPermission {
    resource: String,
    sensitivity: Option<kanzei_harness::permission::FileSensitivity>,
}

pub(super) struct PermissionPlan {
    action: &'static str,
    resolved: Vec<ResolvedPermission>,
    pub(super) pending_ask: Vec<PendingPermission>,
    pub(super) denied: Option<String>,
}

impl PermissionPlan {
    pub(super) fn resolve_session(
        &mut self,
        approved: &std::collections::HashSet<(String, String)>,
        rules: &[(String, String)],
    ) {
        self.pending_ask.retain(|pending| {
            if let Some(source) = session_approval_source(
                self.action,
                &pending.resource,
                pending.sensitivity,
                approved,
                rules,
            ) {
                self.resolved.push(ResolvedPermission {
                    resource: pending.resource.clone(),
                    decision: "allow",
                    source,
                    rule: None,
                });
                false
            } else {
                true
            }
        });
    }

    pub(super) fn emit_resolved(&self, id: &str, on_event: &mut (dyn FnMut(RunEvent) + Send)) {
        for resolved in &self.resolved {
            on_event(RunEvent::PermissionResolved {
                tool_call_id: id.to_owned(),
                action: self.action.to_owned(),
                resource: resolved.resource.clone(),
                decision: resolved.decision,
                source: resolved.source,
                rule: resolved.rule.clone(),
            });
        }
    }
}

fn session_approval_source(
    action: &str,
    resource: &str,
    sensitivity: Option<kanzei_harness::permission::FileSensitivity>,
    approved: &std::collections::HashSet<(String, String)>,
    rules: &[(String, String)],
) -> Option<&'static str> {
    if approved.contains(&(action.to_owned(), resource.to_owned())) {
        Some("session_approved")
    } else if rules.iter().any(|(known_action, pattern)| {
        known_action == action
            && (sensitivity.is_none() || !pattern.contains('*'))
            && kanzei_harness::permission::resource_match_for_action(action, pattern, resource)
    }) {
        Some("session_rule")
    } else {
        None
    }
}

pub(super) fn execution_permissions(
    snapshot: &HarnessSnapshot,
    approved: &std::collections::HashSet<(String, String)>,
    rules: &[(String, String)],
) -> kanzei_harness::permission::Ruleset {
    let mut permissions = snapshot.permissions().clone();
    for (action, resource) in rules.iter().chain(approved.iter()) {
        permissions.push(kanzei_harness::rule(action, resource, Effect::Allow));
    }
    permissions
}

/// Resolve immutable rules once. Serial execution handles pending user asks;
/// parallel execution consumes this exact plan only when no ask remains.
pub(super) fn permission_plan(
    snapshot: &HarnessSnapshot,
    tool: &dyn Tool,
    input: &serde_json::Value,
    ctx: &ToolCtx,
    policy: kanzei_harness::ExecutionPolicy,
) -> PermissionPlan {
    let action = tool.action();
    let mut plan = PermissionPlan {
        action,
        resolved: Vec::new(),
        pending_ask: Vec::new(),
        denied: None,
    };
    if policy == kanzei_harness::ExecutionPolicy::ReadOnly
        && (tool.read_only_support() == kanzei_harness::tool::ReadOnlySupport::Unsupported
            || (tool.read_only_support() != kanzei_harness::tool::ReadOnlySupport::ControlPlane
                && !matches!(
                    tool.concurrency(input, ctx),
                    kanzei_harness::ToolConcurrency::Shared(_)
                )))
    {
        plan.denied = Some(format!("read-only execution scope forbids {}", tool.name()));
        return plan;
    }
    for resource in tool.permission_resources(input, ctx) {
        use kanzei_harness::permission::{FileResource, Resource};
        let (resource, effect, rule, sensitivity) = match resource {
            Resource::Literal(resource) => {
                let resource =
                    kanzei_harness::permission::normalize_resource_for_action(action, &resource);
                let (effect, rule) = snapshot.evaluate_with_rule(action, &resource);
                (resource, effect, rule, None)
            }
            Resource::File(path) => {
                if let Err(error) = kanzei_harness::permission::ScopedFile::resolve(
                    &ctx.cwd,
                    &ctx.project_root,
                    std::path::Path::new(&path),
                ) {
                    plan.resolved.push(ResolvedPermission {
                        resource: path.clone(),
                        decision: "deny",
                        source: "workspace_boundary",
                        rule: None,
                    });
                    plan.denied = Some(format!("{path} ({error})"));
                    plan.pending_ask.clear();
                    break;
                }
                let file = match FileResource::resolve(&ctx.cwd, std::path::Path::new(&path)) {
                    Ok(file) => file,
                    Err(error) => {
                        plan.resolved.push(ResolvedPermission {
                            resource: path.clone(),
                            decision: "deny",
                            source: "filesystem_resolution",
                            rule: None,
                        });
                        plan.denied = Some(format!(
                            "{path} (cannot resolve filesystem resource: {error})"
                        ));
                        plan.pending_ask.clear();
                        break;
                    }
                };
                let (effect, rule) = snapshot
                    .permissions()
                    .evaluate_file_with_rule(action, &file);
                (file.name.clone(), effect, rule, file.sensitivity())
            }
        };
        match effect {
            Effect::Ask => plan.pending_ask.push(PendingPermission {
                resource,
                sensitivity,
            }),
            Effect::Allow | Effect::Deny => {
                let denied = effect == Effect::Deny;
                plan.resolved.push(ResolvedPermission {
                    resource: resource.clone(),
                    decision: if denied { "deny" } else { "allow" },
                    source: "ruleset",
                    rule: rule.map(super::describe_rule),
                });
                if denied {
                    plan.denied = Some(resource);
                    plan.pending_ask.clear();
                    break;
                }
            }
        }
    }
    plan
}

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
    let plan = permission_plan(
        state.snapshot,
        state.tool,
        state.input,
        state.ctx,
        state.config.execution_policy,
    );
    plan.emit_resolved(state.id, state.on_event);
    let mut gate_result = plan.denied.map_or(Gate::Pass, Gate::Deny);
    if matches!(gate_result, Gate::Pass) {
        for pending in plan.pending_ask {
            let resource = pending.resource;
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
            if let Some(source) = session_approval_source(
                action,
                &resource,
                pending.sensitivity,
                state.session_approved,
                state.session_rules,
            ) {
                resolved("allow", source);
                continue;
            }
            match state.config.ask_policy {
                // D-281:自动放行——权限询问直接放行并落事件,不短路、
                // 不再需要前端替答(前端 07-events.js 只处理 Interactive 轮)。
                AskPolicy::AutoAllow if pending.sensitivity.is_none() => {
                    resolved("allow", "auto_allow");
                    continue;
                }
                _ if !state.config.ask_policy.allows_permission_prompt() => {
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
                // 本会话内同样只记**完全相同的资源**(generalize_resource 不做通配),与写进项目配置的口径一致。
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
