//! Harness 装配:组件 → 草稿(六注册表)→ 不可变快照。

use std::path::PathBuf;
use std::sync::Arc;

use crate::config::KanzeiConfig;
use crate::context::ContextSource;
use crate::defs::{AgentDef, CommandDef, ProfileKind, SkillDef};
use crate::permission::{Effect, Rule, Ruleset};
use crate::registry::Registry;
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
    pub agents: Registry<AgentDef>,
    pub tools: Registry<Arc<dyn Tool>>,
    pub commands: Registry<CommandDef>,
    pub skills: Registry<SkillDef>,
    pub context: Registry<Arc<dyn ContextSource>>,
    pub permissions: Ruleset,
}

pub struct Harness {
    components: Vec<Box<dyn Component>>,
}

impl Default for Harness {
    fn default() -> Self {
        Harness {
            components: Vec::new(),
        }
    }
}

impl Harness {
    pub fn add(&mut self, component: impl Component + 'static) -> &mut Self {
        self.components.push(Box::new(component));
        self
    }

    /// 组件按注册顺序贡献;后注册的组件对同名条目 last-wins。
    pub fn resolve(&self, ctx: &ResolveCtx) -> anyhow::Result<Arc<HarnessSnapshot>> {
        let mut draft = HarnessDraft::default();
        for component in &self.components {
            component.contribute(&mut draft, ctx)?;
        }
        Ok(Arc::new(HarnessSnapshot {
            ctx: ctx.clone(),
            draft,
        }))
    }
}

pub struct HarnessSnapshot {
    ctx: ResolveCtx,
    draft: HarnessDraft,
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

    pub fn commands(&self) -> &Registry<CommandDef> {
        &self.draft.commands
    }

    pub fn skills(&self) -> &Registry<SkillDef> {
        &self.draft.skills
    }

    /// 选 agent:显式名 → 无则该 profile 的第一个 primary agent。
    pub fn select_agent(&self, name: Option<&str>) -> anyhow::Result<&AgentDef> {
        if let Some(name) = name {
            return self.draft.agents.get(name).ok_or_else(|| {
                anyhow::anyhow!(
                    "unknown agent `{name}`; available: {}",
                    self.draft.agents.names().collect::<Vec<_>>().join(", ")
                )
            });
        }
        self.draft
            .agents
            .iter()
            .map(|(_, a)| a)
            .find(|a| {
                a.profile.includes(self.ctx.profile) && a.mode == crate::defs::AgentMode::Primary
            })
            .ok_or_else(|| anyhow::anyhow!("no primary agent for profile {:?}", self.ctx.profile))
    }

    /// 工具物化:整体 deny 的 action 直接摘掉(模型根本看不见,硬门禁第一层)。
    pub fn materialize_tools(&self) -> Vec<Arc<dyn Tool>> {
        self.draft
            .tools
            .iter()
            .filter(|(name, _)| !self.draft.permissions.action_fully_denied(name))
            .map(|(_, t)| Arc::clone(t))
            .collect()
    }

    /// 渲染 system baseline:各 Context Source 依注册顺序拼接。
    pub fn system_baseline(&self) -> String {
        let mut sections = Vec::new();
        for (_, src) in self.draft.context.iter() {
            if let Some(text) = src.baseline(&self.ctx) {
                let text = text.trim();
                if !text.is_empty() {
                    sections.push(text.to_string());
                }
            }
        }
        sections.join("\n\n")
    }

    /// 权限评估入口(拦截器在 core 侧调用)。
    pub fn evaluate(&self, action: &str, resource: &str) -> Effect {
        self.draft.permissions.evaluate(action, resource)
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
