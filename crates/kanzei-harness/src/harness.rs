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

#[derive(Default)]
pub struct Harness {
    components: Vec<Box<dyn Component>>,
}

impl Harness {
    pub fn add(&mut self, component: impl Component + 'static) -> &mut Self {
        self.components.push(Box::new(component));
        self
    }

    /// 组件按注册顺序贡献;后注册的组件对同名条目 last-wins。
    ///
    /// 装配末尾做**能力覆盖校验**(D-173):声明了 required_tool 的托管资源族,
    /// 那个工具必须真的在注册表里,否则拒绝理由会指向一个不存在的工具,
    /// 模型照做失败后就会转去找旁路。这是装配期的编程错误,直接炸在启动。
    pub fn resolve(&self, ctx: &ResolveCtx) -> anyhow::Result<Arc<HarnessSnapshot>> {
        let mut draft = HarnessDraft::default();
        for component in &self.components {
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
        self.system_baseline_with_report().0
    }

    /// baseline + 上下文账单(R-106):每个 source 注入了多少字符,
    /// "本轮上下文里有什么、各占多少"从此是数据而不是猜测。
    pub fn system_baseline_with_report(&self) -> (String, Vec<(String, usize)>) {
        let mut sections = Vec::new();
        let mut report = Vec::new();
        for (name, src) in self.draft.context.iter() {
            if let Some(text) = src.baseline(&self.ctx) {
                let text = text.trim();
                if !text.is_empty() {
                    report.push((name.to_string(), text.chars().count()));
                    sections.push(text.to_string());
                }
            }
        }
        (sections.join("\n\n"), report)
    }

    /// 权限评估入口(拦截器在 core 侧调用)。
    pub fn evaluate(&self, action: &str, resource: &str) -> Effect {
        self.draft.permissions.evaluate(action, resource)
    }

    /// 档位权限快照(R-102):列出每个已注册工具的整体决策。
    ///
    /// 快照语义 = 该工具在 `*` 资源上的最终效果(Allow/Deny/Ask),以及它是否被
    /// 整体摘除(硬 deny 会让模型根本看不见这个工具)。批3 的档位权限快照测试
    /// 用它做断言:readonly 档位下 write/edit/bash 必须是 Deny,read/glob/grep/task 放行。
    pub fn permission_snapshot(&self) -> Vec<PermissionSnapshot> {
        permission_snapshot_of(&self.draft)
    }

    /// 被硬门禁拒绝时回喂模型的下一步指引(D-173)。
    ///
    /// 三种情形必须说三种话:有专用工具就点名它;声明为"暂无专用工具"的资源族
    /// 必须如实说成能力未实现,并明确堵死绕行;不属于任何托管族的普通 deny
    /// 保持原样。旧文案一律说 "use the dedicated tool for it",在架构索引这类
    /// 没有工具的资源上就是假信息——模型照做失败后转而用 shell 绕过。
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
                    Some(tool) => format!(
                        "This resource is policy-managed{note}. The ONLY legal write channel is \
                         the `{tool}` tool — use it instead. Do not route around this gate."
                    ),
                    None => format!(
                        "This resource is policy-managed{note} and NO dedicated tool exists for \
                         it yet — this is an unimplemented capability, not a permission you can \
                         work around. Do NOT reach it through the shell (redirects, \
                         Set-Content/Out-File, [System.IO.File]::WriteAllText, python/node \
                         one-liners, git checkout of a crafted blob): shell writes to this path \
                         are detected and rolled back. Record the capability gap (`defect add`) \
                         and tell the user, then move on."
                    ),
                }
            }
            None => format!(
                "`{action}` on this resource is denied by the ruleset. If you believe it should \
                 be allowed, say so to the user — do not look for another route to the same effect."
            ),
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
    /// 硬 deny 会让工具整体摘除——模型根本看不见它。
    pub fully_denied: bool,
}

/// 快照遍历逻辑:HarnessSnapshot::permission_snapshot 与测试共用同一实现。
fn permission_snapshot_of(draft: &HarnessDraft) -> Vec<PermissionSnapshot> {
    let mut names: Vec<String> = draft.tools.iter().map(|(n, _)| n.to_string()).collect();
    // task 是 runner 内建子代理工具,不在工具注册表里;补进快照让只读档位的
    // "task 放行" 可被同一条断言覆盖。
    names.push("task".into());
    names.sort();
    names
        .into_iter()
        .map(|action| {
            let fully_denied = draft.permissions.action_fully_denied(&action);
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

        let snap = permission_snapshot_of(&draft);
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
}
