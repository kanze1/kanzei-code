//! Harness 装配：组件 → 草稿(四类注册表与权限规则)→ 不可变快照。

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use crate::config::KanzeiConfig;
use crate::context::{
    ContextAssemblyError, ContextBaseline, ContextRequirement, ContextSource, ContextSourceReport,
    ContextSourceStatus,
};
use crate::defs::{AgentDef, ProfileKind, SkillDef};
use crate::permission::{Effect, Rule, Ruleset};
use crate::registry::{RegistrationReport, Registry};
use crate::tool::Tool;

/// 一次 resolve 的输入环境。
#[derive(Clone)]
pub struct ResolveCtx {
    pub profile: ProfileKind,
    pub cwd: PathBuf,
    /// 项目根(.kanzei / .git 所在),工作区文档挂在这下面。
    pub project_root: PathBuf,
    pub config: Arc<KanzeiConfig>,
}

pub trait Component: Send + Sync {
    fn contribute(&self, draft: &mut HarnessDraft, ctx: &ResolveCtx) -> anyhow::Result<()>;
}

#[derive(Default)]
pub struct HarnessDraft {
    pub agent_sources: Vec<crate::agent_sources::AgentSource>,
    pub agents: Registry<AgentDef>,
    pub tools: Registry<Arc<dyn Tool>>,
    /// R-364 B2:延迟目录中的工具名仍留在完整执行表里,只影响模型首轮 specs。
    pub deferred_tools: BTreeSet<String>,
    pub skills: Registry<SkillDef>,
    pub context: Registry<Arc<dyn ContextSource>>,
    pub permissions: Ruleset,
}

#[derive(Default)]
pub struct Harness {
    components: Vec<(&'static str, Box<dyn Component>)>,
}

impl Harness {
    pub fn add(&mut self, component: impl Component + 'static) -> &mut Self {
        self.components
            .push((std::any::type_name_of_val(&component), Box::new(component)));
        self
    }

    /// 组件按注册顺序贡献;后注册的组件对同名条目 last-wins。
    ///
    /// 装配末尾做**能力覆盖校验**(D-173):声明了 required_tool 的托管资源族,
    /// 那个工具必须真的在注册表里,否则拒绝理由会指向一个不存在的工具,
    /// 模型照做失败后就会转去找旁路。这是装配期的编程错误,直接炸在启动。
    pub fn resolve(&self, ctx: &ResolveCtx) -> anyhow::Result<Arc<HarnessSnapshot>> {
        let mut draft = HarnessDraft::default();
        for (name, component) in &self.components {
            draft.agents.set_component(name);
            draft.tools.set_component(name);
            draft.skills.set_component(name);
            draft.context.set_component(name);
            component.contribute(&mut draft, ctx)?;
        }
        let missing: Vec<String> = draft
            .permissions
            .managed_resources()
            .iter()
            .filter_map(|managed| {
                let tool = managed.required_tool.as_deref()?;
                (draft.tools.get(tool).is_none())
                    .then(|| format!("{} {} → `{tool}`", managed.action, managed.resource))
            })
            .collect();
        if !missing.is_empty() {
            anyhow::bail!(
                "harness 装配错误:以下硬 deny 资源族声明的专用工具没有注册,拒绝理由会指向不存在的工具:\n{}",
                missing.join("\n")
            );
        }
        let unregistered_deferred: Vec<String> = draft
            .deferred_tools
            .iter()
            .filter(|name| draft.tools.get(name.as_str()).is_none())
            .cloned()
            .collect();
        if !unregistered_deferred.is_empty() {
            anyhow::bail!(
                "harness 装配错误:延迟工具名单包含未注册工具:\n{}",
                unregistered_deferred.join("\n")
            );
        }
        if draft
            .deferred_tools
            .contains(crate::tool_search::TOOL_SEARCH)
        {
            anyhow::bail!("harness 装配错误:tool_search 不能列入延迟工具名单");
        }
        let snapshot = Arc::new(HarnessSnapshot {
            ctx: ctx.clone(),
            draft,
        });
        tracing::debug!(registrations = ?snapshot.registry_report(), "harness registry resolved");
        Ok(snapshot)
    }
}

pub struct HarnessSnapshot {
    ctx: ResolveCtx,
    draft: HarnessDraft,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct HarnessRegistryReport {
    pub agent_sources: Vec<crate::agent_sources::AgentSource>,
    pub agents: Vec<RegistrationReport>,
    pub tools: Vec<RegistrationReport>,
    pub skills: Vec<RegistrationReport>,
    pub context: Vec<RegistrationReport>,
}

impl HarnessSnapshot {
    pub fn profile(&self) -> ProfileKind {
        self.ctx.profile
    }

    pub fn config(&self) -> &KanzeiConfig {
        &self.ctx.config
    }

    pub fn permissions(&self) -> &Ruleset {
        &self.draft.permissions
    }

    pub fn agents(&self) -> &Registry<AgentDef> {
        &self.draft.agents
    }

    pub fn agent_sources(&self) -> &[crate::agent_sources::AgentSource] {
        &self.draft.agent_sources
    }

    pub fn skills(&self) -> &Registry<SkillDef> {
        &self.draft.skills
    }

    /// 顶层选择只接受当前 profile 的 Primary;子代理名册由子代理运行时独立管理。
    pub fn select_primary_agent(&self, name: Option<&str>) -> anyhow::Result<&AgentDef> {
        let candidates: Vec<_> = self
            .draft
            .agents
            .iter()
            .filter(|(_, a)| {
                a.profile.includes(self.ctx.profile) && a.mode == crate::defs::AgentMode::Primary
            })
            .collect();
        let candidate_names = candidates
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join(", ");
        let available = if candidate_names.is_empty() {
            "(none)"
        } else {
            &candidate_names
        };
        if let Some(name) = name {
            let agent = self.draft.agents.get(name).ok_or_else(|| anyhow::anyhow!(
                "unknown primary agent `{name}` for profile {:?}; available primary agents: {available}", self.ctx.profile
            ))?;
            anyhow::ensure!(
                agent.profile.includes(self.ctx.profile) && agent.mode == crate::defs::AgentMode::Primary,
                "agent `{name}` has scope {:?} and mode {:?}; it cannot be a primary agent for profile {:?}; available primary agents: {available}",
                agent.profile, agent.mode, self.ctx.profile
            );
            return Ok(agent);
        }
        candidates.first().map(|(_, agent)| *agent).ok_or_else(|| {
            anyhow::anyhow!(
                "no primary agent for profile {:?}; available primary agents: {available}",
                self.ctx.profile
            )
        })
    }

    /// 可观察的装配结果;与运行时注册表同源,不重新扫描配置目录推测胜者。
    pub fn registry_report(&self) -> HarnessRegistryReport {
        HarnessRegistryReport {
            agent_sources: self.draft.agent_sources.clone(),
            agents: self.draft.agents.report(),
            tools: self.draft.tools.report(),
            skills: self.draft.skills.report(),
            context: self.draft.context.report(),
        }
    }

    /// Materialize tools admitted by both the ruleset and profile capability boundary.
    pub fn materialize_tools(&self) -> Vec<Arc<dyn Tool>> {
        self.draft
            .tools
            .iter()
            .filter(|(name, _)| !self.draft.permissions.action_fully_denied(name))
            .filter(|(_, tool)| profile_allows_tool(self.ctx.profile, tool.as_ref()))
            .map(|(_, t)| Arc::clone(t))
            .collect()
    }

    /// R-364 B2:only defer tools when the loader itself is model-visible. A denied
    /// `tool_search` falls back to the unchanged, fully materialized tool surface.
    fn deferred_loading_enabled(&self) -> bool {
        self.draft
            .tools
            .get(crate::tool_search::TOOL_SEARCH)
            .is_some()
            && !self
                .draft
                .permissions
                .action_fully_denied(crate::tool_search::TOOL_SEARCH)
    }

    /// Whether a registered, permitted tool belongs to the deferred layer.
    pub fn is_deferred(&self, name: &str) -> bool {
        self.deferred_loading_enabled()
            && self.draft.deferred_tools.contains(name)
            && self
                .draft
                .tools
                .get(name)
                .is_some_and(|tool| profile_allows_tool(self.ctx.profile, tool.as_ref()))
            && !self.draft.permissions.action_fully_denied(name)
    }

    /// All permitted tools declared deferred, in registry order.
    pub fn deferred_tools(&self) -> Vec<Arc<dyn Tool>> {
        if !self.deferred_loading_enabled() {
            return Vec::new();
        }
        self.materialize_tools()
            .into_iter()
            .filter(|tool| self.draft.deferred_tools.contains(tool.name()))
            .collect()
    }

    /// All permitted, initially visible tools, in registry order.
    pub fn resident_tools(&self) -> Vec<Arc<dyn Tool>> {
        self.materialize_tools()
            .into_iter()
            .filter(|tool| !self.is_deferred(tool.name()))
            .collect()
    }

    /// The compact prompt directory for deferred tools, if any.
    pub fn deferred_catalog(&self) -> Option<String> {
        crate::tool_search::render_catalog(&self.deferred_tools())
    }

    /// 渲染 system baseline:各 Context Source 依注册顺序拼接。
    pub fn system_baseline(&self) -> Result<String, ContextAssemblyError> {
        self.system_baseline_with_report().map(|(text, _)| text)
    }

    /// baseline + 上下文账单(R-106):每个 source 注入了多少字符,
    /// "本轮上下文里有什么、各占多少"从此是数据而不是猜测。
    pub fn system_baseline_with_report(&self) -> ContextBaseline {
        let (mut stable, mut report) = self.stable_system_baseline_with_report()?;
        let (refreshing, refreshing_report) = self.refreshable_system_baseline_with_report()?;
        if !refreshing.is_empty() {
            if !stable.is_empty() {
                stable.push_str("\n\n");
            }
            stable.push_str(&refreshing);
        }
        report.extend(refreshing_report);
        Ok((stable, report))
    }

    /// 只渲染一次的稳定上下文。runner 用它保持既有 baseline 快照语义。
    pub fn stable_system_baseline_with_report(&self) -> ContextBaseline {
        self.render_context_sources(false)
    }

    /// 每个模型步骤前替换的临时上下文;不进入 messages,因此不会逐轮累积。
    pub fn refreshable_system_baseline_with_report(&self) -> ContextBaseline {
        self.render_context_sources(true)
    }

    fn render_context_sources(&self, refreshing: bool) -> ContextBaseline {
        let mut sections = Vec::new();
        let mut report = Vec::new();
        for (name, src) in self.draft.context.iter() {
            if src.refresh_each_step() != refreshing {
                continue;
            }
            let mut entry = ContextSourceReport {
                key: name.to_string(),
                requirement: src.requirement(),
                refresh_each_step: refreshing,
                status: ContextSourceStatus::Empty,
                chars: 0,
                bytes: 0,
                error: None,
            };
            match src.baseline(&self.ctx) {
                Ok(Some(text)) if !text.trim().is_empty() => {
                    let text = text.trim();
                    entry.status = ContextSourceStatus::Loaded;
                    entry.chars = text.chars().count();
                    entry.bytes = text.len();
                    sections.push(text.to_string());
                }
                Ok(_) => {}
                Err(error) => {
                    entry.status = ContextSourceStatus::Failed;
                    entry.error = Some(format!("{error:#}"));
                }
            }
            report.push(entry);
        }
        if report.iter().any(|entry| {
            entry.requirement == ContextRequirement::Required
                && entry.status == ContextSourceStatus::Failed
        }) {
            return Err(ContextAssemblyError { sources: report });
        }
        Ok((sections.join("\n\n"), report))
    }

    /// Resource ruleset evaluation. An Allow does not bypass capability admission;
    /// core separately checks readonly support and the operation's access scope.
    pub fn evaluate(&self, action: &str, resource: &str) -> Effect {
        self.draft.permissions.evaluate(action, resource)
    }

    /// R-183:与 [`evaluate`] 同一判定,额外返回命中的规则原文(可审计轨迹用)。
    pub fn evaluate_with_rule(
        &self,
        action: &str,
        resource: &str,
    ) -> (Effect, Option<&crate::permission::Rule>) {
        self.draft.permissions.evaluate_with_rule(action, resource)
    }

    /// Profile permissions include unavailable readonly capabilities. Mixed tools
    /// still require per-operation admission, which cannot be inferred from `*`.
    ///
    /// Reports the wildcard resource effect and whole-tool exclusion; it does
    /// not claim that every operation of a mixed tool is executable.
    pub fn permission_snapshot(&self) -> Vec<PermissionSnapshot> {
        permission_snapshot_of(&self.draft, self.ctx.profile)
    }

    /// Describe the actual managed owner and its visibility in this execution scope.
    pub fn denial_hint(&self, action: &str, resource: &str) -> String {
        match self.draft.permissions.managed_for(action, resource) {
            Some(managed) => {
                if managed.note_only {
                    return managed.note.clone().unwrap_or_else(|| {
                        format!("`{action}` on this resource is disabled by the current setting.")
                    });
                }
                let note = managed
                    .note
                    .as_deref()
                    .map(|n| format!(" ({n})"))
                    .unwrap_or_default();
                match managed.required_tool.as_deref() {
                    Some(tool) if !self.materialize_tools().iter().any(|candidate| candidate.name() == tool) => format!(
                        "This resource is policy-managed{note}. Its owning tool `{tool}` is unavailable in this execution scope."
                    ),
                    Some(tool) if self.is_deferred(tool) => format!(
                        "This resource is policy-managed{note}; writes use `{tool}`. Its schema is available through tool_search query `select:{tool}`; direct calls also load it."
                    ),
                    Some(tool) => format!("This resource is policy-managed{note}; writes use `{tool}`."),
                    None => format!("This resource is policy-managed{note}; no owning write tool is registered."),
                }
            }
            None => format!("The current ruleset denies `{action}` on this resource."),
        }
    }
}

/// 把 kanzei.toml 的权限规则等贡献进草稿的内置组件。
pub struct ConfigComponent;

impl Component for ConfigComponent {
    fn contribute(&self, draft: &mut HarnessDraft, ctx: &ResolveCtx) -> anyhow::Result<()> {
        draft
            .permissions
            .extend(ctx.config.permissions.rules.iter().cloned());
        Ok(())
    }
}

/// 便捷构造:权限规则。
pub fn rule(action: &str, resource: &str, effect: Effect) -> Rule {
    Rule {
        action: action.into(),
        resource: resource.into(),
        effect,
    }
}

/// 档位权限快照的一项(R-102):某工具在 `*` 资源上的整体决策。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionSnapshot {
    pub action: String,
    pub effect: Effect,
    /// Excluded by a hard deny or the profile capability boundary.
    pub fully_denied: bool,
}

/// Profile tool visibility is shared by materialization and permission diagnostics.
fn profile_allows_tool(profile: ProfileKind, tool: &dyn Tool) -> bool {
    profile != ProfileKind::Readonly
        || tool.read_only_support() != crate::tool::ReadOnlySupport::Unsupported
}

fn permission_snapshot_of(draft: &HarnessDraft, profile: ProfileKind) -> Vec<PermissionSnapshot> {
    let mut names: Vec<String> = draft.tools.iter().map(|(n, _)| n.to_string()).collect();
    // The runner owns task registration; this row reports its resource policy,
    // not whether delegation is installed or a writable role is eligible.
    names.push("task".into());
    names.sort();
    names
        .into_iter()
        .map(|action| {
            let fully_denied = draft.permissions.action_fully_denied(&action)
                || draft
                    .tools
                    .get(&action)
                    .is_some_and(|tool| !profile_allows_tool(profile, tool.as_ref()));
            let effect = if fully_denied {
                Effect::Deny
            } else {
                draft.permissions.evaluate(&action, "*")
            };
            PermissionSnapshot {
                action,
                effect,
                fully_denied,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TestTool(&'static str, crate::tool::ReadOnlySupport);

    #[async_trait::async_trait]
    impl Tool for TestTool {
        fn read_only_support(&self) -> crate::tool::ReadOnlySupport {
            self.1
        }
        fn name(&self) -> &'static str {
            self.0
        }
        fn description(&self) -> String {
            format!("{} tool.", self.0)
        }
        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({"type":"object", "properties":{}})
        }
        async fn execute(
            &self,
            _input: serde_json::Value,
            _ctx: &crate::ToolCtx,
        ) -> crate::ToolOutput {
            crate::ToolOutput::ok("unused")
        }
    }

    fn resolve_ctx() -> ResolveCtx {
        let root = PathBuf::from("C:/r364-harness-test");
        ResolveCtx {
            profile: ProfileKind::DevAuto,
            cwd: root.clone(),
            project_root: root,
            config: Arc::new(KanzeiConfig::default()),
        }
    }

    fn test_tool(name: &'static str) -> Arc<dyn Tool> {
        Arc::new(TestTool(name, crate::tool::ReadOnlySupport::Unsupported))
    }

    #[test]
    fn readonly_capabilities_match_initial_tools_deferred_catalog_and_permission_report() {
        use crate::tool::ReadOnlySupport;
        struct Catalog;
        impl Component for Catalog {
            fn contribute(&self, draft: &mut HarnessDraft, _: &ResolveCtx) -> anyhow::Result<()> {
                for (name, support) in [
                    ("observation", ReadOnlySupport::ReadOnly),
                    ("control", ReadOnlySupport::ControlPlane),
                    ("mutation", ReadOnlySupport::Unsupported),
                ] {
                    draft
                        .tools
                        .insert(name, Arc::new(TestTool(name, support)) as Arc<dyn Tool>);
                    draft.deferred_tools.insert(name.into());
                    draft.permissions.push(rule(name, "*", Effect::Allow));
                }
                draft.tools.insert(
                    crate::tool_search::TOOL_SEARCH,
                    Arc::new(crate::ToolSearchTool) as Arc<dyn Tool>,
                );
                Ok(())
            }
        }
        let mut ctx = resolve_ctx();
        ctx.profile = ProfileKind::Readonly;
        let mut harness = Harness::default();
        harness.add(Catalog);
        let snapshot = harness.resolve(&ctx).unwrap();
        let tools = snapshot.materialize_tools();
        assert!(!tools.iter().any(|tool| tool.name() == "mutation"));
        assert!(tools.iter().any(|tool| tool.name() == "observation"));
        assert!(tools.iter().any(|tool| tool.name() == "control"));
        assert!(!snapshot.is_deferred("mutation"));
        let catalog = snapshot.deferred_catalog().unwrap();
        assert!(!catalog.contains("mutation"));
        assert!(catalog.contains("observation"));
        let report = snapshot.permission_snapshot();
        let mutation = report
            .iter()
            .find(|entry| entry.action == "mutation")
            .unwrap();
        assert!(mutation.fully_denied);
        assert_eq!(mutation.effect, Effect::Deny);
    }

    #[test]
    fn explicit_primary_selection_checks_scope_and_role() {
        let mut draft = HarnessDraft::default();
        for (name, profile, mode) in [
            (
                "dev-only",
                crate::ProfileScope::Development,
                crate::AgentMode::Primary,
            ),
            (
                "research-only",
                crate::ProfileScope::Research,
                crate::AgentMode::Primary,
            ),
            (
                "all-primary",
                crate::ProfileScope::All,
                crate::AgentMode::Primary,
            ),
            (
                "scout",
                crate::ProfileScope::All,
                crate::AgentMode::Subagent,
            ),
        ] {
            draft.agents.insert(
                name,
                AgentDef {
                    name: name.into(),
                    profile,
                    mode,
                    model: "primary".into(),
                    steps: 0,
                    system: String::new(),
                },
            );
        }
        let mut snapshot = HarnessSnapshot {
            ctx: resolve_ctx(),
            draft,
        };
        for profile in [
            ProfileKind::DevAuto,
            ProfileKind::DevPair,
            ProfileKind::Research,
            ProfileKind::Readonly,
        ] {
            snapshot.ctx.profile = profile;
            assert_eq!(
                snapshot
                    .select_primary_agent(Some("all-primary"))
                    .unwrap()
                    .name,
                "all-primary"
            );
            assert_eq!(
                snapshot.select_primary_agent(Some("dev-only")).is_ok(),
                profile.is_development()
            );
            assert_eq!(
                snapshot.select_primary_agent(Some("research-only")).is_ok(),
                profile == ProfileKind::Research
            );
            let error = snapshot
                .select_primary_agent(Some("scout"))
                .unwrap_err()
                .to_string();
            assert!(error.contains("primary"), "{error}");
            assert!(error.contains("all-primary"), "{error}");
            assert_eq!(
                snapshot.select_primary_agent(None).unwrap().mode,
                crate::AgentMode::Primary
            );
        }
        let unknown = snapshot
            .select_primary_agent(Some("missing"))
            .unwrap_err()
            .to_string();
        assert!(unknown.contains("all-primary"), "{unknown}");
        assert!(!unknown.contains("dev-only"), "{unknown}");
        assert!(!unknown.contains("scout"), "{unknown}");
    }

    #[test]
    fn deferred_tools_only_hide_from_resident_specs_when_loader_is_visible() {
        let mut draft = HarnessDraft::default();
        draft.tools.insert("resident", test_tool("resident"));
        draft
            .tools
            .insert(crate::tool_search::TOOL_SEARCH, test_tool("tool_search"));
        draft.tools.insert("deferred", test_tool("deferred"));
        draft.deferred_tools.insert("deferred".into());
        let snapshot = HarnessSnapshot {
            ctx: resolve_ctx(),
            draft,
        };

        let names =
            |tools: Vec<Arc<dyn Tool>>| tools.iter().map(|tool| tool.name()).collect::<Vec<_>>();
        assert_eq!(
            names(snapshot.materialize_tools()),
            ["resident", "tool_search", "deferred"]
        );
        assert_eq!(
            names(snapshot.resident_tools()),
            ["resident", "tool_search"]
        );
        assert_eq!(names(snapshot.deferred_tools()), ["deferred"]);
        assert!(snapshot.is_deferred("deferred"));
        assert!(snapshot
            .deferred_catalog()
            .unwrap()
            .contains("deferred — deferred tool."));
        let catalog = snapshot.deferred_catalog().unwrap();
        assert!(catalog.contains("Load schemas with tool_search"));
        assert!(catalog.contains("select:name1,name2"));
        assert!(catalog.contains("direct calls also auto-load"));
    }

    #[test]
    fn denial_hint_points_to_tool_search_for_deferred_managed_tool() {
        let mut draft = HarnessDraft::default();
        draft
            .tools
            .insert(crate::tool_search::TOOL_SEARCH, test_tool("tool_search"));
        draft.tools.insert("deferred", test_tool("deferred"));
        draft.deferred_tools.insert("deferred".into());
        draft.permissions.push_managed_hard_deny(
            crate::rule("edit", "*.kanzei/project/*", Effect::Deny),
            Some("deferred"),
            Some("managed test resource"),
        );
        let snapshot = HarnessSnapshot {
            ctx: resolve_ctx(),
            draft,
        };

        let hint = snapshot.denial_hint("edit", "C:/project/.kanzei/project/item.md");
        assert!(hint.contains("tool_search"), "{hint}");
        assert!(hint.contains("select:deferred"), "{hint}");
    }

    #[test]
    fn denial_hint_matches_actual_dedicated_channel_visibility() {
        struct ManagedTools(bool);
        impl Component for ManagedTools {
            fn contribute(
                &self,
                draft: &mut HarnessDraft,
                _ctx: &ResolveCtx,
            ) -> anyhow::Result<()> {
                draft
                    .tools
                    .insert("managed_write", test_tool("managed_write"));
                draft.permissions.push_managed_hard_deny(
                    rule("write", "*.kanzei/project/*", Effect::Deny),
                    Some("managed_write"),
                    Some("managed test resource"),
                );
                if self.0 {
                    draft
                        .tools
                        .insert(crate::TOOL_SEARCH, test_tool("tool_search"));
                    draft.deferred_tools.insert("managed_write".into());
                }
                Ok(())
            }
        }

        for deferred in [false, true] {
            for exception in [None, Some(Effect::Allow), Some(Effect::Ask)] {
                let mut config = KanzeiConfig::default();
                config
                    .permissions
                    .rules
                    .push(rule("managed_write", "*", Effect::Deny));
                if let Some(effect) = exception {
                    config
                        .permissions
                        .rules
                        .push(rule("managed_write", "write:allowed", effect));
                }
                let ctx = ResolveCtx {
                    config: Arc::new(config),
                    ..resolve_ctx()
                };
                let mut harness = Harness::default();
                harness.add(ManagedTools(deferred)).add(ConfigComponent);
                let snapshot = harness.resolve(&ctx).unwrap();
                let visible = snapshot
                    .materialize_tools()
                    .iter()
                    .any(|tool| tool.name() == "managed_write");
                let hint = snapshot.denial_hint("write", "C:/project/.kanzei/project/item.md");
                assert_eq!(visible, exception.is_some());
                assert_eq!(
                    hint.contains("unavailable in this execution scope"),
                    !visible,
                    "{hint}"
                );
                if !visible {
                    assert!(!hint.contains("use it instead"), "{hint}");
                    assert!(!hint.contains("select:managed_write"), "{hint}");
                    assert_eq!(
                        snapshot.evaluate("managed_write", "write:allowed"),
                        Effect::Deny
                    );
                } else if deferred {
                    assert!(hint.contains("select:managed_write"), "{hint}");
                } else {
                    assert!(hint.contains("writes use `managed_write`"), "{hint}");
                }
            }
        }
    }

    #[test]
    fn managed_denial_does_not_offer_a_tool_excluded_by_readonly_scope() {
        let mut draft = HarnessDraft::default();
        draft
            .tools
            .insert("managed_write", test_tool("managed_write"));
        draft.permissions.push_managed_hard_deny(
            rule("write", "*.kanzei/project/*", Effect::Deny),
            Some("managed_write"),
            None,
        );
        let mut ctx = resolve_ctx();
        ctx.profile = ProfileKind::Readonly;
        let snapshot = HarnessSnapshot { ctx, draft };
        assert!(snapshot.materialize_tools().is_empty());
        let hint = snapshot.denial_hint("write", "C:/project/.kanzei/project/item.md");
        assert!(
            hint.contains("unavailable in this execution scope"),
            "{hint}"
        );
        for extra in ["writes use", "select:managed_write", "defect add", "Retry"] {
            assert!(!hint.contains(extra), "{hint}");
        }
    }

    #[test]
    fn denied_tool_search_falls_back_to_all_materialized_tools() {
        let mut draft = HarnessDraft::default();
        draft.tools.insert("resident", test_tool("resident"));
        draft
            .tools
            .insert(crate::tool_search::TOOL_SEARCH, test_tool("tool_search"));
        draft.tools.insert("deferred", test_tool("deferred"));
        draft.deferred_tools.insert("deferred".into());
        draft.permissions.push_hard_deny(crate::rule(
            crate::tool_search::TOOL_SEARCH,
            "*",
            Effect::Deny,
        ));
        let snapshot = HarnessSnapshot {
            ctx: resolve_ctx(),
            draft,
        };

        assert_eq!(snapshot.deferred_tools().len(), 0);
        assert!(snapshot.deferred_catalog().is_none());
        assert!(!snapshot.is_deferred("deferred"));
        assert_eq!(snapshot.resident_tools().len(), 2);
    }

    struct InvalidDeferred(&'static str);
    impl Component for InvalidDeferred {
        fn contribute(&self, draft: &mut HarnessDraft, _ctx: &ResolveCtx) -> anyhow::Result<()> {
            draft.deferred_tools.insert(self.0.into());
            Ok(())
        }
    }

    #[test]
    fn resolve_rejects_unregistered_or_deferred_tool_search_names() {
        for name in ["missing", crate::tool_search::TOOL_SEARCH] {
            let mut harness = Harness::default();
            harness.add(InvalidDeferred(name));
            match harness.resolve(&resolve_ctx()) {
                Err(error) => assert!(error.to_string().contains("延迟")),
                Ok(_) => panic!("invalid deferred name `{name}` unexpectedly resolved"),
            }
        }
    }

    #[test]
    fn permission_snapshot_reflects_ruleset_and_hard_denies() {
        let mut draft = HarnessDraft::default();
        // 普通规则:read 放行、bash ask。
        draft.permissions.push(rule("read", "*", Effect::Allow));
        draft.permissions.push(rule("bash", "*", Effect::Ask));
        // 硬 deny:write 整体摘除。
        draft
            .permissions
            .push_hard_deny(rule("write", "*", Effect::Deny));
        // 模拟已注册工具(快照遍历的是 tools 注册表)。
        struct DummyTool;
        #[async_trait::async_trait]
        impl Tool for DummyTool {
            fn name(&self) -> &'static str {
                "read"
            }
            fn description(&self) -> String {
                String::new()
            }
            fn input_schema(&self) -> serde_json::Value {
                serde_json::json!({})
            }
            async fn execute(
                &self,
                _input: serde_json::Value,
                _ctx: &crate::ToolCtx,
            ) -> crate::ToolOutput {
                unimplemented!()
            }
        }
        draft.tools.insert("read", Arc::new(DummyTool));
        draft.tools.insert("bash", Arc::new(DummyTool));
        draft.tools.insert("write", Arc::new(DummyTool));

        let snap = permission_snapshot_of(&draft, ProfileKind::DevAuto);
        let read = snap.iter().find(|s| s.action == "read").unwrap();
        assert_eq!(read.effect, Effect::Allow);
        assert!(!read.fully_denied);
        let bash = snap.iter().find(|s| s.action == "bash").unwrap();
        assert_eq!(bash.effect, Effect::Ask);
        let write = snap.iter().find(|s| s.action == "write").unwrap();
        assert_eq!(write.effect, Effect::Deny);
        assert!(write.fully_denied);
        // task 不在注册表但快照补了它(只读档位的 task 放行断言依赖它)。
        let task = snap.iter().find(|s| s.action == "task").unwrap();
        assert_eq!(task.effect, Effect::Ask);
    }

    #[test]
    fn refreshable_context_is_separate_and_recomputed_without_accumulation() {
        struct ContextComponent {
            value: Arc<AtomicUsize>,
        }
        impl Component for ContextComponent {
            fn contribute(
                &self,
                draft: &mut HarnessDraft,
                _ctx: &ResolveCtx,
            ) -> anyhow::Result<()> {
                draft.context.insert(
                    "stable",
                    crate::source("stable", |_| Some("stable-value".into())),
                );
                let value = self.value.clone();
                draft.context.insert(
                    "refreshing",
                    crate::refreshing_source("refreshing", move |_| {
                        Some(format!("dynamic-{}", value.load(Ordering::SeqCst)))
                    }),
                );
                Ok(())
            }
        }

        let value = Arc::new(AtomicUsize::new(1));
        let mut harness = Harness::default();
        harness.add(ContextComponent {
            value: value.clone(),
        });
        let snapshot = harness
            .resolve(&ResolveCtx {
                profile: ProfileKind::DevAuto,
                cwd: PathBuf::from("C:/test"),
                project_root: PathBuf::from("C:/test"),
                config: Arc::new(KanzeiConfig::default()),
            })
            .unwrap();
        assert_eq!(
            snapshot.stable_system_baseline_with_report().unwrap().0,
            "stable-value"
        );
        assert_eq!(
            snapshot
                .refreshable_system_baseline_with_report()
                .unwrap()
                .0,
            "dynamic-1"
        );
        value.store(2, Ordering::SeqCst);
        assert_eq!(
            snapshot
                .refreshable_system_baseline_with_report()
                .unwrap()
                .0,
            "dynamic-2",
            "刷新只能给出新值,不能把 dynamic-1 累加回来"
        );
        let combined = snapshot.system_baseline().unwrap();
        assert!(combined.contains("stable-value"));
        assert_eq!(combined.matches("dynamic-2").count(), 1);
    }
}
